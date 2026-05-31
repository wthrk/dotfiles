//! `SshAgentPort` を gpg-agent の SSH key list（`sshcontrol`）と SSH agent socket 観測へ接続する adapter。
//!
//! authentication subkey の keygrip を `${GNUPGHOME:-$HOME/.gnupg}/sshcontrol` へ冪等に登録し、SSH support
//! 利用可否を「SSH agent socket（`S.gpg-agent.ssh`）が解決でき、keygrip が登録済みである」状態として観測
//! して domain 値（`SshAgentReadiness`）へ翻訳する。`gpgconf` CLI は使わず、socket path は
//! `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を優先候補として確認する。SSH support 充足の業務判定
//! そのものは domain（`SshAgentReadiness::ensure_ready`）へ残す。

use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

use anyhow::Context;

use crate::{
    Result,
    secrets::{
        domain::gpg_restore::{Keygrip, SshAgentReadiness},
        ports::gpg::SshAgentPort,
    },
};

/// gpg-agent の SSH key list と socket 観測を `SshAgentPort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct SshAgentAdapter;

impl SshAgentPort for SshAgentAdapter {
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()> {
        let path = sshcontrol_path()?;
        if sshcontrol_contains(&path, keygrip)? {
            // 既登録ならその状態を維持する（冪等）。
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("failed to create GnuPG home directory for sshcontrol")?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .context("failed to open gpg-agent sshcontrol")?;
        writeln!(file, "{}", keygrip.as_str())
            .context("failed to register keygrip in gpg-agent sshcontrol")?;
        Ok(())
    }

    fn inspect_ssh_agent(&mut self, keygrip: &Keygrip) -> Result<SshAgentReadiness> {
        let socket_resolved = ssh_agent_socket_path()?
            .map(|path| is_socket(&path))
            .unwrap_or(false);
        let authentication_identity_present = sshcontrol_contains(&sshcontrol_path()?, keygrip)?;
        Ok(SshAgentReadiness {
            socket_resolved,
            authentication_identity_present,
        })
    }
}

/// `${GNUPGHOME:-$HOME/.gnupg}` を解決する。
fn gnupg_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("GNUPGHOME") {
        return Ok(PathBuf::from(home));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; cannot resolve GnuPG home")?;
    Ok(PathBuf::from(home).join(".gnupg"))
}

/// gpg-agent の SSH key list（`sshcontrol`）の path を返す。
fn sshcontrol_path() -> Result<PathBuf> {
    Ok(gnupg_home()?.join("sshcontrol"))
}

/// `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を SSH agent socket の優先候補として返す。
fn ssh_agent_socket_path() -> Result<Option<PathBuf>> {
    Ok(Some(gnupg_home()?.join("S.gpg-agent.ssh")))
}

/// 指定 path が socket として存在するかを返す。
fn is_socket(path: &PathBuf) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.file_type().is_socket())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// `sshcontrol` に keygrip（uppercase hex 40）が既に登録されているかを返す。
fn sshcontrol_contains(path: &PathBuf, keygrip: &Keygrip) -> Result<bool> {
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(anyhow::Error::new(error).context("failed to read gpg-agent sshcontrol"));
        }
    };
    for line in BufReader::new(file).lines() {
        let line = line.context("failed to read gpg-agent sshcontrol line")?;
        let entry = line.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        // sshcontrol の各行は keygrip（uppercase hex）で始まる。大文字小文字を無視して照合する。
        if entry.eq_ignore_ascii_case(keygrip.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

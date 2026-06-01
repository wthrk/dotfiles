//! `GitClonePort` を git2 + libssh2 の SSH agent 認証 clone へ接続する adapter。
//!
//! private `password-store` repository を `~/.password-store` へ clone する。認証は git2 の credentials
//! callback で libssh2 の SSH agent 経路（`Cred::ssh_key_from_agent`）を使い、gpg-agent の SSH support
//! が提示する GPG authentication subkey 由来の identity を利用する。`git` CLI と GitHub API は使わず、
//! SSH agent socket は #14 と同じく `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を優先候補として解決し、
//! socket でなければ既存 `SSH_AUTH_SOCK` を維持する（`config/zsh/env.zsh` の上書き条件と同じ前提）。
//! clone URL の妥当性判断は domain（`PasswordStoreRemote`）に委ね、ここでは git2 への翻訳だけを担う。

use std::path::PathBuf;

use anyhow::Context;
use git2::{Cred, CredentialType, FetchOptions, RemoteCallbacks, build::RepoBuilder};

use crate::{
    Result,
    secrets::{adapters::git::password_store_path, domain::pass_restore::PasswordStoreRemote},
};

/// git2 の SSH agent 認証 clone を `GitClonePort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct GitCloneAdapter;

impl GitCloneAdapter {
    /// 検証済み clone URL を `~/.password-store` へ SSH agent 認証で clone する。
    pub(super) fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()> {
        // SSH agent 経路の利用には socket が解決できる必要がある。解決できなければ clone を試みず停止する。
        let socket = resolve_ssh_agent_socket()
            .context("could not resolve a gpg-agent SSH agent socket for password-store clone")?;
        // libssh2 は credentials callback で SSH agent を使う前に `SSH_AUTH_SOCK` を参照する。#14 と同じ
        // socket 解決結果へ環境変数を合わせ、`git2` が同じ SSH agent 経路を使うようにする。
        // SAFETY: clone 実行は単一スレッドの use case 経路であり、ここでの環境変数設定は本 process の
        // SSH agent 接続先を #14 の解決結果へ固定するためだけに行う。
        unsafe {
            std::env::set_var("SSH_AUTH_SOCK", &socket);
        }

        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, allowed_types| {
            // SSH の username は GitHub では `git` 固定。URL から取れない場合も `git` を使う。
            let username = username_from_url.unwrap_or("git");
            if allowed_types.contains(CredentialType::SSH_KEY) {
                // GPG authentication subkey 由来 identity は gpg-agent の SSH agent から提示される。
                Cred::ssh_key_from_agent(username)
            } else {
                Err(git2::Error::from_str(
                    "password-store clone requires SSH agent authentication",
                ))
            }
        });

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        let destination = password_store_path()?;
        RepoBuilder::new()
            .fetch_options(fetch_options)
            .clone(remote.as_str(), &destination)
            .map_err(|error| {
                anyhow::anyhow!("failed to clone private password-store over SSH: {error}")
            })?;
        Ok(())
    }
}

/// SSH agent socket を解決する。
///
/// `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket ならそれを優先し、socket でない場合だけ既存
/// `SSH_AUTH_SOCK` が socket ならそれへ fallback する（#14 の `ssh_agent_adapter` と同じ解決規則）。
/// `gpgconf` CLI は使わない。
fn resolve_ssh_agent_socket() -> Option<PathBuf> {
    if let Ok(home) = gnupg_home() {
        let fixed = home.join("S.gpg-agent.ssh");
        if is_socket(&fixed) {
            return Some(fixed);
        }
    }
    if let Some(env) = std::env::var_os("SSH_AUTH_SOCK") {
        let env = PathBuf::from(env);
        if is_socket(&env) {
            return Some(env);
        }
    }
    None
}

/// `${GNUPGHOME:-$HOME/.gnupg}` を解決する。
fn gnupg_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("GNUPGHOME") {
        return Ok(PathBuf::from(home));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; cannot resolve GnuPG home")?;
    Ok(PathBuf::from(home).join(".gnupg"))
}

/// 指定 path が socket として存在するかを返す。
fn is_socket(path: &std::path::Path) -> bool {
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

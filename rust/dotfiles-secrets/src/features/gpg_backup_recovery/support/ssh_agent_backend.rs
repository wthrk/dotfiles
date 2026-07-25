//! gpg-agent SSH concrete backend operations.
use crate::{
    Result,
    features::gpg_backup_recovery::domain::gpg_restore::{
        Keygrip, OpenSshPublicKey, SshAgentReadiness,
    },
    features::gpg_backup_recovery::support::{
        gpg_host_security,
        ssh_agent_protocol::{
            request_identities, sshcontrol_contains, sshcontrol_line_matches_keygrip,
            sshcontrol_path,
        },
        ssh_agent_socket::{gnupg_home, resolve_ssh_agent_socket},
    },
};
use anyhow::Context;
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub(crate) fn register_authentication_subkey(keygrip: &Keygrip) -> Result<bool> {
    gpg_host_security::ensure_gnupg_host_security()?;
    let path = sshcontrol_path(gnupg_home()?);
    if sshcontrol_contains(&path, keygrip)? {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create GnuPG home directory for sshcontrol")?;
    }
    let mut file = open_owner_only_sshcontrol_for_append(&path)?;
    // 新設・既存のどちらでも append 直前に GNUPGHOME 全体を再検査する。作成直後の `sshcontrol` の
    // owner/type/mode と、親 directory / private-key material の安全境界を同じ host preflight で確認する。
    gpg_host_security::ensure_gnupg_host_security()?;
    writeln!(file, "{}", keygrip.as_str())
        .context("failed to register keygrip in gpg-agent sshcontrol")?;
    Ok(true)
}

/// `sshcontrol` を新設する場合は umask に依存せず 0600 を明示し、append 前に同じ host preflight を再実行する。
///
/// 既存 file は caller の事前 preflight で検査済みでも、open と write の間の path replacement を fail-closed に
/// するため、ここでも owner/type/mode を検査する。
fn open_owner_only_sshcontrol_for_append(path: &Path) -> Result<std::fs::File> {
    match OpenOptions::new()
        .append(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .context("failed to set owner-only mode on new gpg-agent sshcontrol")?;
            gpg_host_security::ensure_sshcontrol_file_security(path)?;
            Ok(file)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            gpg_host_security::ensure_sshcontrol_file_security(path)?;
            OpenOptions::new()
                .append(true)
                .open(path)
                .context("failed to open gpg-agent sshcontrol")
        }
        Err(error) => {
            Err(anyhow::Error::new(error).context("failed to create gpg-agent sshcontrol"))
        }
    }
}

/// rollback で invocation-owned `sshcontrol` entry だけを除去する。
pub(crate) fn unregister_authentication_subkey(keygrip: &Keygrip) -> Result<()> {
    gpg_host_security::ensure_gnupg_host_security()?;
    let path = sshcontrol_path(gnupg_home()?);
    let contents = std::fs::read_to_string(&path)
        .context("failed to read gpg-agent sshcontrol for rollback")?;
    let retained = contents
        .lines()
        .filter(|line| !sshcontrol_line_matches_keygrip(line, keygrip))
        .collect::<Vec<_>>()
        .join("\n");
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .context("failed to open gpg-agent sshcontrol for rollback")?;
    if !retained.is_empty() {
        writeln!(file, "{retained}")
            .context("failed to remove invocation-owned keygrip from gpg-agent sshcontrol")?;
    }
    Ok(())
}
pub(crate) fn inspect_ssh_agent(
    expected_public_key: &OpenSshPublicKey,
) -> Result<SshAgentReadiness> {
    gpg_host_security::ensure_gnupg_host_security()?;
    let socket = resolve_ssh_agent_socket()?;
    let socket_resolved = socket.is_some();
    let recovery_identity_present = match socket {
        Some(path) => request_identities(&path)
            .context("failed to inspect resolved SSH agent identities")?
            .iter()
            .any(|blob| expected_public_key.matches_agent_key_blob(blob)),
        None => false,
    };
    Ok(SshAgentReadiness {
        socket_resolved,
        recovery_identity_present,
    })
}

#[cfg(test)]
mod tests {
    //! 新設 `sshcontrol` の mode と既存 unsafe file の fail-closed 境界を検査する。

    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::{Path, PathBuf},
    };

    use super::open_owner_only_sshcontrol_for_append;

    fn fixture_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dotfiles-ssh-agent-backend-{name}-{}",
            std::process::id()
        ))
    }

    fn remove_fixture(path: &Path) -> crate::Result<()> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
            Ok(_) => fs::remove_file(path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    /// OpenOptions の create mode に加え post-create chmod を行うため、umask に依存せず 0600 を強制する。
    #[test]
    fn new_sshcontrol_is_explicitly_owner_read_write() -> crate::Result<()> {
        let home = fixture_path("new-mode");
        remove_fixture(&home)?;
        fs::create_dir(&home)?;
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
        let path = home.join("sshcontrol");

        drop(open_owner_only_sshcontrol_for_append(&path)?);
        assert_eq!(fs::symlink_metadata(&path)?.mode() & 0o777, 0o600);

        remove_fixture(&home)
    }

    /// 既存の group-readable `sshcontrol` は append 前の再検査で拒否する。
    #[test]
    fn existing_unsafe_sshcontrol_is_rejected_before_append() -> crate::Result<()> {
        let home = fixture_path("unsafe-existing");
        remove_fixture(&home)?;
        fs::create_dir(&home)?;
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
        let path = home.join("sshcontrol");
        fs::write(&path, b"fixture\\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;

        assert!(open_owner_only_sshcontrol_for_append(&path).is_err());
        remove_fixture(&home)
    }
}

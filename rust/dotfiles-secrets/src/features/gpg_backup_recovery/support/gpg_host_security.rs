//! GnuPG home と gpg-agent control path の owner-only filesystem preflight。
//!
//! target host 上の passphrase-free software key は、YubiKey touch と BWS envelope に加え、
//! owner-only GnuPG filesystem boundary を必要とする。これは repository 固有の recovery policy を
//! 決める module ではなく、given path の ownership、symlink、mode、file type を technical facts として
//! fail-closed に検査する support backend である。
//!
//! `std::os::unix::fs::MetadataExt` と rustix 1.1.4
//! [`process::geteuid`](https://docs.rs/rustix/1.1.4/rustix/process/fn.geteuid.html) を使い、
//! current effective UID と metadata UID、group/world の全 access bits、symlink/path replacement を検査する。
//! I/O/metadata error は unsafe/safe へ推測変換せず source error のまま停止する。

use std::{
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::Path,
};

use anyhow::Context;

use crate::{Result, features::gpg_backup_recovery::support::ssh_agent_socket::gnupg_home};

const GROUP_OR_WORLD_ACCESS: u32 = 0o077;

/// GnuPG home と、存在する repository-owned control paths が current user 専用であることを確認する。
pub(crate) fn ensure_gnupg_host_security() -> Result<()> {
    let home = gnupg_home()?;
    ensure_owner_only_directory(&home, "GNUPGHOME")?;
    ensure_optional_owner_only_directory(&home.join("private-keys-v1.d"), "private-keys-v1.d")?;
    ensure_private_key_material(&home.join("private-keys-v1.d"))?;
    ensure_optional_owner_only_regular_file(&home.join("sshcontrol"), "sshcontrol")?;
    ensure_optional_owner_only_socket(&home.join("S.gpg-agent.ssh"), "gpg-agent SSH socket")
}

/// 作成直後を含む `sshcontrol` が owner-only regular file であることを確認する。
///
/// 作成側は append より前にこの検査を再実行し、umask や path replacement による unsafe な file への
/// 書き込みを許可しない。
pub(crate) fn ensure_sshcontrol_file_security(path: &Path) -> Result<()> {
    ensure_optional_owner_only_regular_file(path, "sshcontrol")
}

fn expected_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

fn metadata_without_symlink(path: &Path, label: &str) -> Result<std::fs::Metadata> {
    metadata_without_symlink_for_uid(path, label, expected_uid())
}

/// path を follow せずに検査し、指定 effective UID に属する owner-only object だけを返す。
///
/// production caller は current effective UID を渡す。UID を引数化するのは policy を変更するためではなく、
/// test が実 filesystem の owner を変更せず wrong-owner boundary を直接確認するためである。
fn metadata_without_symlink_for_uid(
    path: &Path,
    label: &str,
    required_uid: u32,
) -> Result<std::fs::Metadata> {
    let metadata = path
        .symlink_metadata()
        .with_context(|| format!("failed to inspect {label}"))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!("{label} must not be a symlink");
    }
    if metadata.uid() != required_uid {
        anyhow::bail!("{label} is not owned by the current user");
    }
    if metadata.permissions().mode() & GROUP_OR_WORLD_ACCESS != 0 {
        anyhow::bail!("{label} is accessible by group or world");
    }
    Ok(metadata)
}

fn ensure_owner_only_directory(path: &Path, label: &str) -> Result<()> {
    let metadata = metadata_without_symlink(path, label)?;
    if !metadata.is_dir() {
        anyhow::bail!("{label} must be a directory");
    }
    if metadata.permissions().mode() & 0o700 != 0o700 {
        anyhow::bail!("{label} must be owner-readable, writable, and searchable");
    }
    Ok(())
}

fn ensure_private_key_material(path: &Path) -> Result<()> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(anyhow::Error::new(error).context("failed to scan private key material"));
        }
    };
    for entry in entries {
        let entry = entry.context("failed to inspect private key material entry")?;
        let label = format!(
            "private key material {}",
            entry.file_name().to_string_lossy()
        );
        let metadata = metadata_without_symlink(&entry.path(), &label)?;
        if !metadata.is_file() {
            anyhow::bail!("{label} must be a regular file");
        }
        if metadata.permissions().mode() & 0o600 != 0o600 {
            anyhow::bail!("{label} must be owner-readable and writable");
        }
    }
    Ok(())
}

fn ensure_optional_owner_only_directory(path: &Path, label: &str) -> Result<()> {
    match path.symlink_metadata() {
        Ok(_) => ensure_owner_only_directory(path, label),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::Error::new(error).context(format!("failed to inspect {label}"))),
    }
}

fn ensure_optional_owner_only_regular_file(path: &Path, label: &str) -> Result<()> {
    match path.symlink_metadata() {
        Ok(_) => {
            let metadata = metadata_without_symlink(path, label)?;
            if !metadata.is_file() {
                anyhow::bail!("{label} must be a regular file");
            }
            if metadata.permissions().mode() & 0o600 != 0o600 {
                anyhow::bail!("{label} must be owner-readable and writable");
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::Error::new(error).context(format!("failed to inspect {label}"))),
    }
}

fn ensure_optional_owner_only_socket(path: &Path, label: &str) -> Result<()> {
    match path.symlink_metadata() {
        Ok(_) => {
            let metadata = metadata_without_symlink(path, label)?;
            if !metadata.file_type().is_socket() {
                anyhow::bail!("{label} must be a socket");
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::Error::new(error).context(format!("failed to inspect {label}"))),
    }
}

#[cfg(test)]
mod tests {
    //! owner-only preflight が group/world access と symlink を fail-closed に拒否する。

    use std::{
        fs,
        os::unix::{fs::PermissionsExt, net::UnixListener},
        path::PathBuf,
    };

    use super::{
        GROUP_OR_WORLD_ACCESS, ensure_optional_owner_only_regular_file,
        ensure_optional_owner_only_socket, ensure_owner_only_directory,
        ensure_private_key_material, metadata_without_symlink_for_uid,
    };

    fn fixture_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dotfiles-gpg-host-security-{name}-{}",
            std::process::id()
        ))
    }

    fn remove_fixture(path: &std::path::Path) -> crate::Result<()> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
            Ok(_) => fs::remove_file(path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    #[test]
    fn owner_only_policy_rejects_all_group_world_access() {
        assert_eq!(0o600 & GROUP_OR_WORLD_ACCESS, 0);
        assert_ne!(0o644 & GROUP_OR_WORLD_ACCESS, 0);
        assert_ne!(0o755 & GROUP_OR_WORLD_ACCESS, 0);
    }

    #[test]
    fn absent_optional_file_is_accepted() -> crate::Result<()> {
        let path = fixture_path("missing");
        remove_fixture(&path)?;
        ensure_optional_owner_only_regular_file(&path, "fixture")
    }

    /// `0755` GNUPGHOME は group/world execute を許すため、secret key material の親として拒否する。
    #[test]
    fn directory_with_group_or_world_access_is_rejected() -> crate::Result<()> {
        let path = fixture_path("directory-mode");
        remove_fixture(&path)?;
        fs::create_dir(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        let result = ensure_owner_only_directory(&path, "fixture");
        remove_fixture(&path)?;
        assert!(result.is_err());
        Ok(())
    }

    /// `0644` の private-key file は group/world read を許すため、scan 中に拒否する。
    #[test]
    fn private_file_with_group_or_world_access_is_rejected() -> crate::Result<()> {
        let path = fixture_path("file-mode");
        remove_fixture(&path)?;
        fs::write(&path, b"fixture")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        let result = ensure_optional_owner_only_regular_file(&path, "fixture");
        remove_fixture(&path)?;
        assert!(result.is_err());
        Ok(())
    }

    /// symlink は検査後の path replacement を許すため、owner-only mode でも受け入れない。
    #[test]
    fn symlink_is_rejected() -> crate::Result<()> {
        let target = fixture_path("symlink-target");
        let link = fixture_path("symlink");
        remove_fixture(&target)?;
        remove_fixture(&link)?;
        fs::write(&target, b"fixture")?;
        std::os::unix::fs::symlink(&target, &link)?;
        let result = ensure_optional_owner_only_regular_file(&link, "fixture");
        remove_fixture(&link)?;
        remove_fixture(&target)?;
        assert!(result.is_err());
        Ok(())
    }

    /// euid と異なる owner は、mode が owner-only に見えても受け入れない。
    #[test]
    fn wrong_owner_is_rejected_without_changing_fixture_owner() -> crate::Result<()> {
        let path = fixture_path("wrong-owner");
        remove_fixture(&path)?;
        fs::write(&path, b"fixture")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let actual_uid = std::os::unix::fs::MetadataExt::uid(&fs::symlink_metadata(&path)?);
        let result = metadata_without_symlink_for_uid(&path, "fixture", actual_uid.wrapping_add(1));
        remove_fixture(&path)?;
        assert!(result.is_err());
        Ok(())
    }

    /// `private-keys-v1.d` の scan は file mode を個別に検査し、親 directory だけで許可しない。
    #[test]
    fn private_key_scan_rejects_inaccessible_file() -> crate::Result<()> {
        let path = fixture_path("private-key-scan");
        remove_fixture(&path)?;
        fs::create_dir(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        let key = path.join("fixture.key");
        fs::write(&key, b"fixture")?;
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644))?;
        let result = ensure_private_key_material(&path);
        remove_fixture(&path)?;
        assert!(result.is_err());
        Ok(())
    }

    /// agent path に regular file を置いて socket を偽装しても、agent backend の前に停止する。
    #[test]
    fn agent_socket_path_rejects_regular_file() -> crate::Result<()> {
        let path = fixture_path("agent-regular-file");
        remove_fixture(&path)?;
        fs::write(&path, b"fixture")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let result = ensure_optional_owner_only_socket(&path, "fixture");
        remove_fixture(&path)?;
        assert!(result.is_err());
        Ok(())
    }

    /// owner-only UNIX socket は gpg-agent control path として通し、socket type 検査を確認する。
    #[test]
    fn owner_only_agent_socket_is_accepted() -> crate::Result<()> {
        let path = fixture_path("agent-socket");
        remove_fixture(&path)?;
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        ensure_optional_owner_only_socket(&path, "fixture")?;
        drop(listener);
        remove_fixture(&path)
    }
}

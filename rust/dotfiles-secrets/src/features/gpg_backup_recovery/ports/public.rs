//! Cross-feature capability contracts owned by `gpg_backup_recovery`.
pub(crate) use super::gpg::{BackupCipherPort, GpgKeyringPort, SshAgentPort};
#[cfg(test)]
pub(crate) use super::gpg::{MockBackupCipherPort, MockGpgKeyringPort, MockSshAgentPort};
pub(crate) use crate::features::gpg_backup_recovery::application::{
    add_spare::run_add_gpg_backup_spare,
    export_ssh_public_key::run_export_ssh_public_key,
    register_primary::{RegisterGpgBackupYubikeyRuntime, run_register_gpg_backup_primary},
    restore_gpg::{RestoreGpgIdentityRuntime, RestoreGpgYubikeyRuntime, run_restore_gpg},
    validate_source::run_validate_gpg_backup_source,
};
pub(crate) use crate::features::gpg_backup_recovery::domain::{
    commands::{
        AddGpgBackupSpareCommand, ExportSshPublicKeyCommand, RegisterGpgBackupCommand,
        RestoreGpgCommand,
    },
    gpg_backup::{
        BackupUpdateGuard, ConnectedYubiKey, EnvelopeRecipient, GpgBackupEnvelope,
        PrimaryFingerprint,
    },
    gpg_restore::{OpenSshPublicKey, RestoreGpgSummary},
};

/// password-store SSH clone が使う strict gpg-agent socket capability。
///
/// socket の解決規則と owner-only preflight は GPG feature が所有し、consumer は support module や
/// environment policy を直接参照しない。
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) fn resolve_strict_gpg_ssh_agent_socket() -> crate::Result<Option<std::path::PathBuf>> {
    crate::features::gpg_backup_recovery::support::gpg_host_security::ensure_gnupg_host_security()?;
    crate::features::gpg_backup_recovery::support::ssh_agent_socket::resolve_gpg_agent_socket()
}

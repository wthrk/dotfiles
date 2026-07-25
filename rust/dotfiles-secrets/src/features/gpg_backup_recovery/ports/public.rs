//! Cross-feature capability contracts owned by `gpg_backup_recovery`.

pub(crate) use super::gpg::{BackupCipherPort, GpgAgentSocketPort, GpgKeyringPort, SshAgentPort};
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

//! gpgme keyring port の forwarding-only adapter。
use crate::{
    Result,
    features::gpg_backup_recovery::ports::gpg::GpgKeyringPort,
    features::gpg_backup_recovery::support::gpg_keyring_backend,
    features::{
        gpg_backup_recovery::domain::{
            gpg_backup::PrimaryFingerprint,
            gpg_restore::{ImportedKeyComposition, Keygrip, OpenPgpBackupFacts, OpenSshPublicKey},
        },
        password_store::ports::public::GpgRecipientId,
    },
    foundation::protection::ProtectedSecret,
    shared::contracts::adapter_backend::GpgKeyringBackend,
};
impl GpgKeyringPort for GpgKeyringBackend {
    fn export_secret_key(&mut self, primary: &PrimaryFingerprint) -> Result<ProtectedSecret> {
        gpg_keyring_backend::export_secret_key(primary)
    }
    fn parse_backup_primary_fingerprint(
        &mut self,
        backup: &ProtectedSecret,
    ) -> Result<OpenPgpBackupFacts> {
        gpg_keyring_backend::parse_backup_primary_fingerprint(backup)
    }
    fn secret_key_exists(&mut self, primary: &PrimaryFingerprint) -> Result<bool> {
        gpg_keyring_backend::secret_key_exists(primary)
    }
    fn import_secret_key(&mut self, backup: &ProtectedSecret) -> Result<PrimaryFingerprint> {
        gpg_keyring_backend::import_secret_key(backup)
    }
    fn delete_secret_key(&mut self, primary: &PrimaryFingerprint) -> Result<()> {
        gpg_keyring_backend::delete_secret_key(primary)
    }
    fn inspect_imported_key(
        &mut self,
        primary: &PrimaryFingerprint,
    ) -> Result<ImportedKeyComposition> {
        gpg_keyring_backend::inspect_imported_key(primary)
    }
    fn authentication_subkey_keygrip(&mut self, primary: &PrimaryFingerprint) -> Result<Keygrip> {
        gpg_keyring_backend::authentication_subkey_keygrip(primary)
    }
    fn authentication_subkey_ssh_public_key(
        &mut self,
        primary: &PrimaryFingerprint,
    ) -> Result<OpenSshPublicKey> {
        gpg_keyring_backend::authentication_subkey_ssh_public_key(primary)
    }
    fn secret_key_available_for_recipient(&mut self, recipient: &GpgRecipientId) -> Result<bool> {
        gpg_keyring_backend::secret_key_available_for_recipient(recipient)
    }
    fn can_decrypt_store_entry(&mut self, entry_path: &std::path::Path) -> Result<()> {
        gpg_keyring_backend::can_decrypt_store_entry(entry_path)
    }
}

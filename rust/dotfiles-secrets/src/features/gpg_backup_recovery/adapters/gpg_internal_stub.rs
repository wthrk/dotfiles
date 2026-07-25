//! `secrets-internal-test-stub` GPG port の forwarding-only adapter。
use crate::{
    Result,
    features::gpg_backup_recovery::ports::gpg::{BackupCipherPort, GpgKeyringPort, SshAgentPort},
    features::gpg_backup_recovery::support::internal_stub_gpg,
    features::{
        gpg_backup_recovery::domain::{
            gpg_backup::{EnvelopeCiphertext, PrimaryFingerprint},
            gpg_restore::{
                ImportedKeyComposition, Keygrip, OpenPgpBackupFacts, OpenSshPublicKey,
                SshAgentReadiness,
            },
        },
        password_store::ports::public::GpgRecipientId,
    },
    foundation::protection::ProtectedSecret,
    shared::contracts::adapter_backend::{BackupCipherBackend, GpgKeyringBackend, SshAgentBackend},
};
impl GpgKeyringPort for GpgKeyringBackend {
    fn export_secret_key(&mut self, value: &PrimaryFingerprint) -> Result<ProtectedSecret> {
        internal_stub_gpg::export_secret_key(value)
    }
    fn parse_backup_primary_fingerprint(
        &mut self,
        value: &ProtectedSecret,
    ) -> Result<OpenPgpBackupFacts> {
        internal_stub_gpg::parse_backup_primary_fingerprint(value)
    }
    fn secret_key_exists(&mut self, value: &PrimaryFingerprint) -> Result<bool> {
        internal_stub_gpg::secret_key_exists(value)
    }
    fn import_secret_key(&mut self, value: &ProtectedSecret) -> Result<PrimaryFingerprint> {
        internal_stub_gpg::import_secret_key(value)
    }
    fn delete_secret_key(&mut self, value: &PrimaryFingerprint) -> Result<()> {
        internal_stub_gpg::delete_secret_key(value)
    }
    fn inspect_imported_key(
        &mut self,
        value: &PrimaryFingerprint,
    ) -> Result<ImportedKeyComposition> {
        internal_stub_gpg::inspect_imported_key(value)
    }
    fn authentication_subkey_keygrip(&mut self, value: &PrimaryFingerprint) -> Result<Keygrip> {
        internal_stub_gpg::authentication_subkey_keygrip(value)
    }
    fn authentication_subkey_ssh_public_key(
        &mut self,
        value: &PrimaryFingerprint,
    ) -> Result<OpenSshPublicKey> {
        internal_stub_gpg::authentication_subkey_ssh_public_key(value)
    }
    fn secret_key_available_for_recipient(&mut self, value: &GpgRecipientId) -> Result<bool> {
        internal_stub_gpg::secret_key_available_for_recipient(value)
    }
    fn can_decrypt_store_entry(&mut self, value: &std::path::Path) -> Result<()> {
        internal_stub_gpg::can_decrypt_store_entry(value)
    }
}
impl BackupCipherPort for BackupCipherBackend {
    fn generate_dek(&mut self) -> Result<ProtectedSecret> {
        internal_stub_gpg::generate_dek()
    }
    fn encrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        backup: &ProtectedSecret,
    ) -> Result<EnvelopeCiphertext> {
        internal_stub_gpg::encrypt_backup(dek, backup)
    }
    fn decrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        ciphertext: &EnvelopeCiphertext,
    ) -> Result<ProtectedSecret> {
        internal_stub_gpg::decrypt_backup(dek, ciphertext)
    }
}
impl SshAgentPort for SshAgentBackend {
    fn register_authentication_subkey(&mut self, value: &Keygrip) -> Result<bool> {
        internal_stub_gpg::register_authentication_subkey(value)
    }
    fn unregister_authentication_subkey(&mut self, value: &Keygrip) -> Result<()> {
        internal_stub_gpg::unregister_authentication_subkey(value)
    }
    fn inspect_ssh_agent(&mut self, value: &OpenSshPublicKey) -> Result<SshAgentReadiness> {
        internal_stub_gpg::inspect_ssh_agent(value)
    }
}

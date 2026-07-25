//! GPG cipher port の forwarding-only adapter。
use crate::{
    Result, features::gpg_backup_recovery::domain::gpg_backup::EnvelopeCiphertext,
    features::gpg_backup_recovery::ports::gpg::BackupCipherPort,
    features::gpg_backup_recovery::support::gpg_cipher_backend,
    foundation::protection::ProtectedSecret,
    shared::contracts::adapter_backend::BackupCipherBackend,
};
impl BackupCipherPort for BackupCipherBackend {
    fn generate_dek(&mut self) -> Result<ProtectedSecret> {
        gpg_cipher_backend::generate_dek()
    }
    fn encrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        backup: &ProtectedSecret,
    ) -> Result<EnvelopeCiphertext> {
        gpg_cipher_backend::encrypt_backup(dek, backup)
    }
    fn decrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        ciphertext: &EnvelopeCiphertext,
    ) -> Result<ProtectedSecret> {
        gpg_cipher_backend::decrypt_backup(dek, ciphertext)
    }
}

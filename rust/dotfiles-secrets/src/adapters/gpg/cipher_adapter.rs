//! GPG cipher port の forwarding-only adapter。
use crate::{
    Result,
    domain::gpg_backup::EnvelopeCiphertext,
    ports::gpg::BackupCipherPort,
    support::{
        adapter_backend::BackupCipherBackend, gpg_cipher_backend, protection::ProtectedSecret,
    },
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

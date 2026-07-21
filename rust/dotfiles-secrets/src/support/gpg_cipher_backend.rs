//! GPG backup cipher concrete backend operations.

use crate::{
    Result,
    domain::gpg_backup::EnvelopeCiphertext,
    support::protection::{ProtectedSecret, gpg_backup},
};
pub(crate) fn generate_dek() -> Result<ProtectedSecret> {
    gpg_backup::generate_dek()
}
pub(crate) fn encrypt_backup(
    dek: &ProtectedSecret,
    backup: &ProtectedSecret,
) -> Result<EnvelopeCiphertext> {
    let (nonce, body, tag) = gpg_backup::encrypt_backup_body(dek, backup)?;
    EnvelopeCiphertext::new(nonce, body, tag)
}
pub(crate) fn decrypt_backup(
    dek: &ProtectedSecret,
    ciphertext: &EnvelopeCiphertext,
) -> Result<ProtectedSecret> {
    gpg_backup::decrypt_backup_body(dek, ciphertext.nonce(), ciphertext.body(), ciphertext.tag())
}

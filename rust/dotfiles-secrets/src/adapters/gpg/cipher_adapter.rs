//! `BackupCipherPort` を `support/protection` の AES-256-GCM DEK 操作へ接続する adapter。
//!
//! DEK 生成と backup 本体の encrypt/decrypt を support backend へ委譲し、port 境界では envelope
//! `ciphertext`（nonce/body/tag）の domain 値変換だけを担う。envelope schema 検証や recipient 照合の
//! 業務規則は持たない。

use crate::{
    Result,
    domain::gpg_backup::EnvelopeCiphertext,
    ports::gpg::BackupCipherPort,
    support::protection::{ProtectedSecret, gpg_backup},
};

/// AES-256-GCM の DEK 暗復号を `BackupCipherPort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct BackupCipherAdapter;

impl BackupCipherPort for BackupCipherAdapter {
    fn generate_dek(&mut self) -> Result<ProtectedSecret> {
        gpg_backup::generate_dek()
    }

    fn encrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        backup: &ProtectedSecret,
    ) -> Result<EnvelopeCiphertext> {
        let (nonce, body, tag) = gpg_backup::encrypt_backup_body(dek, backup)?;
        EnvelopeCiphertext::new(nonce, body, tag)
    }

    fn decrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        ciphertext: &EnvelopeCiphertext,
    ) -> Result<ProtectedSecret> {
        gpg_backup::decrypt_backup_body(
            dek,
            ciphertext.nonce(),
            ciphertext.body(),
            ciphertext.tag(),
        )
    }
}

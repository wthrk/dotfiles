//! `BackupCipherPort` を `support/protection` の AES-256-GCM DEK 復号操作へ接続する adapter。
//!
//! backup 本体の decrypt を support backend へ委譲し、port 境界では envelope `ciphertext`
//! （nonce/body/tag）の domain 値変換だけを担う。envelope schema 検証や recipient 照合の業務規則は
//! 持たない。

use crate::{
    Result,
    secrets::{
        domain::gpg_backup::EnvelopeCiphertext,
        ports::gpg::BackupCipherPort,
        support::protection::{ProtectedSecret, gpg_backup},
    },
};

/// AES-256-GCM の DEK 暗復号を `BackupCipherPort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct BackupCipherAdapter;

impl BackupCipherPort for BackupCipherAdapter {
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

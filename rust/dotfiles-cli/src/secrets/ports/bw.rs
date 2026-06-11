//! Bitwarden 個人 vault backend へ application が要求する port 契約。
//!
//! この module は vault item 候補取得と secret value 取得/作成の capability だけを宣言し、
//! SDK/API 認証や ID 型変換の詳細を adapter 側へ閉じる。

use super::super::domain::{
    gpg_backup::GpgBackupEnvelope,
    pass_restore::PasswordStoreRemote,
    vault::{BitwardenVaultCredentials, VaultLookupCandidate, VaultSecretId},
};
use crate::Result;

/// use case が Bitwarden 個人 vault API 境界へ要求する契約。
#[cfg_attr(test, mockall::automock)]
pub trait VaultClientPort {
    async fn list_vault_secrets(
        &self,
        credentials: &BitwardenVaultCredentials,
    ) -> Result<Vec<VaultLookupCandidate<VaultSecretId>>>;

    /// `gpg-secret-key-backup` の encrypted envelope を取得する。
    async fn fetch_gpg_backup_envelope(
        &self,
        credentials: &BitwardenVaultCredentials,
        secret_id: &VaultSecretId,
    ) -> Result<GpgBackupEnvelope>;

    /// `password-store-remote` secret value を取得し、GitHub SSH clone URL として domain 検証した値を返す。
    async fn fetch_password_store_remote(
        &self,
        credentials: &BitwardenVaultCredentials,
        secret_id: &VaultSecretId,
    ) -> Result<PasswordStoreRemote>;

    /// 個人 vault に新しい `password-store-remote` secret を作成し、その ID を返す。
    async fn create_password_store_remote(
        &self,
        credentials: &BitwardenVaultCredentials,
        remote: &PasswordStoreRemote,
    ) -> Result<VaultSecretId>;
}

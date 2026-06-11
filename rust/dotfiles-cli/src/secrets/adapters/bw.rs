//! `VaultClientPort` を Bitwarden 個人 vault API 境界へ接続する adapter。
//!
//! application は vault secret の一意解決規則を保持する。adapter は SDK/API の item
//! list/get/create 境界を port の ID 候補と domain 値へ翻訳する。

#[cfg(feature = "secrets-internal-test-stub")]
mod internal_stub;

#[cfg(not(feature = "secrets-internal-test-stub"))]
use std::{future::Future, pin::Pin};

#[cfg(not(feature = "secrets-internal-test-stub"))]
use anyhow::Context;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden_api_api::apis::ciphers_api;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden_api_api::models::{CipherCreateRequestModel, CipherRequestModel};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden_crypto::{KeyDecryptable, KeyEncryptable, SymmetricCryptoKey};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden_vault::{CipherRepromptType, CipherType, CipherView, ClientVaultExt, SyncRequest};

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::{
    Result,
    secrets::{
        domain::{
            gpg_backup::GpgBackupEnvelope,
            pass_restore::PasswordStoreRemote,
            vault::{
                BitwardenVaultCredentials, VaultLookupCandidate, VaultSecretId, VaultSecretName,
            },
        },
        ports::bw::VaultClientPort,
        support::protection::bitwarden_account_api,
    },
};

/// Bitwarden 個人 vault SDK/API を `VaultClientPort` へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct VaultClientAdapter;

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl VaultClientPort for VaultClientAdapter {
    async fn list_vault_secrets(
        &self,
        credentials: &BitwardenVaultCredentials,
    ) -> Result<Vec<VaultLookupCandidate<VaultSecretId>>> {
        Ok(SdkPersonalVaultBackend
            .list(credentials)
            .await?
            .into_iter()
            .map(|item| VaultLookupCandidate {
                id: VaultSecretId::new(item.id),
                name: item.name,
            })
            .collect())
    }

    async fn fetch_gpg_backup_envelope(
        &self,
        credentials: &BitwardenVaultCredentials,
        secret_id: &VaultSecretId,
    ) -> Result<GpgBackupEnvelope> {
        let item = SdkPersonalVaultBackend
            .fetch(credentials, secret_id.as_str())
            .await?;
        GpgBackupEnvelope::from_json(item.value.as_bytes())
    }

    async fn fetch_password_store_remote(
        &self,
        credentials: &BitwardenVaultCredentials,
        secret_id: &VaultSecretId,
    ) -> Result<PasswordStoreRemote> {
        let item = SdkPersonalVaultBackend
            .fetch(credentials, secret_id.as_str())
            .await?;
        PasswordStoreRemote::parse(item.value.as_str())
    }

    async fn create_password_store_remote(
        &self,
        credentials: &BitwardenVaultCredentials,
        remote: &PasswordStoreRemote,
    ) -> Result<VaultSecretId> {
        SdkPersonalVaultBackend
            .create(
                credentials,
                VaultSecretName::PasswordStoreRemote.key(),
                remote.as_str(),
            )
            .await
            .map(VaultSecretId::new)
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
struct PersonalVaultItem {
    id: String,
    name: String,
    value: String,
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// Bitwarden SDK/API backend と port adapter の間で暗号化済み vault item 操作だけを抽象化する境界。
///
/// caller は item 名の一意解決や domain 側の停止判断を保持し、この trait は list/fetch/create の
/// 外部 API 操作を `PersonalVaultItem` へ翻訳する責務に限定する。
trait PersonalVaultBackend {
    fn list<'a>(
        &'a self,
        credentials: &'a BitwardenVaultCredentials,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersonalVaultItem>>> + 'a>>;

    fn fetch<'a>(
        &'a self,
        credentials: &'a BitwardenVaultCredentials,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<PersonalVaultItem>> + 'a>>;

    fn create<'a>(
        &'a self,
        credentials: &'a BitwardenVaultCredentials,
        name: &'a str,
        value: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + 'a>>;
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// Bitwarden SDK/API を使う本番 backend 実装。
///
/// account API key と master password による user crypto 初期化を SDK 境界へ渡し、復号済み view と
/// port/domain 値の間の翻訳だけを行う。
struct SdkPersonalVaultBackend;

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl PersonalVaultBackend for SdkPersonalVaultBackend {
    fn list<'a>(
        &'a self,
        credentials: &'a BitwardenVaultCredentials,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersonalVaultItem>>> + 'a>> {
        Box::pin(async move { load_personal_vault_items(credentials).await })
    }

    fn fetch<'a>(
        &'a self,
        credentials: &'a BitwardenVaultCredentials,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<PersonalVaultItem>> + 'a>> {
        Box::pin(async move {
            load_personal_vault_items(credentials)
                .await?
                .into_iter()
                .find(|item| item.id == id)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Bitwarden personal vault item not found",
                    )
                    .into()
                })
        })
    }

    fn create<'a>(
        &'a self,
        credentials: &'a BitwardenVaultCredentials,
        name: &'a str,
        value: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + 'a>> {
        Box::pin(async move { create_personal_vault_secure_note(credentials, name, value).await })
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// 個人 vault の暗号化済み cipher 一覧を同期し、復号した secure note view を adapter 内部 item へ翻訳する。
///
/// lookup の 0 件・複数件判定は application/domain 側へ残し、この helper は SDK の sync/decrypt 境界と
/// 欠損 SDK field の技術的失敗化だけを扱う。
async fn load_personal_vault_items(
    credentials: &BitwardenVaultCredentials,
) -> Result<Vec<PersonalVaultItem>> {
    let client = bitwarden_account_api::authenticate_with_account_api_key(
        credentials.api_key().client_id(),
        credentials.api_key().client_secret(),
        credentials.master_password(),
    )
    .await?;
    let user_key = SymmetricCryptoKey::try_from(client.crypto().get_user_encryption_key().await?)
        .context("Bitwarden personal vault user key could not be loaded")?;
    let sync = client
        .vault()
        .sync(&SyncRequest {
            exclude_subdomains: Some(true),
        })
        .await
        .context("Bitwarden personal vault sync failed")?;
    sync.ciphers
        .into_iter()
        .filter(|cipher| cipher.deleted_date.is_none())
        .map(|cipher| {
            let view: CipherView = cipher
                .decrypt_with_key(&user_key)
                .context("Bitwarden personal vault item decrypt failed")?;
            Ok(PersonalVaultItem {
                id: view
                    .id
                    .map(|id| id.to_string())
                    .ok_or_else(|| missing_data("Bitwarden personal vault item ID is missing"))?,
                name: view.name,
                value: view.notes.unwrap_or_default(),
            })
        })
        .collect()
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// 個人 vault に secure note cipher を作成し、SDK/API の作成結果 ID を port 境界値へ戻すための helper。
///
/// 保存対象名と値は caller が domain/application 側で決めたものだけを受け取り、この helper は user key による
/// encrypt と Bitwarden API request 変換に責務を閉じる。
async fn create_personal_vault_secure_note(
    credentials: &BitwardenVaultCredentials,
    name: &str,
    value: &str,
) -> Result<String> {
    let client = bitwarden_account_api::authenticate_with_account_api_key(
        credentials.api_key().client_id(),
        credentials.api_key().client_secret(),
        credentials.master_password(),
    )
    .await?;
    let user_key = SymmetricCryptoKey::try_from(client.crypto().get_user_encryption_key().await?)
        .context("Bitwarden personal vault user key could not be loaded")?;
    let now = chrono::Utc::now();
    let cipher = CipherView {
        id: None,
        organization_id: None,
        folder_id: None,
        collection_ids: Vec::new(),
        key: None,
        name: name.to_owned(),
        notes: Some(value.to_owned()),
        r#type: CipherType::SecureNote,
        login: None,
        identity: None,
        card: None,
        secure_note: None,
        favorite: false,
        reprompt: CipherRepromptType::None,
        organization_use_totp: false,
        edit: true,
        view_password: true,
        local_data: None,
        attachments: None,
        fields: None,
        password_history: None,
        creation_date: now,
        deleted_date: None,
        revision_date: now,
    };
    let encrypted = cipher
        .encrypt_with_key(&user_key)
        .context("Bitwarden personal vault item encrypt failed")?;
    let request: CipherRequestModel = serde_json::from_value(serde_json::to_value(encrypted)?)
        .context("Bitwarden personal vault item request encode failed")?;
    let created = client.internal.get_api_configurations().await;
    let created = ciphers_api::ciphers_create_post(
        &created.api,
        Some(CipherCreateRequestModel::new(request)),
    )
    .await
    .context("Bitwarden personal vault item create failed")?;
    created
        .id
        .map(|id| id.to_string())
        .ok_or_else(|| missing_data("Bitwarden personal vault created item ID is missing").into())
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn missing_data(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
/// Bitwarden 個人 vault adapter の inline unit test。
mod tests {
    use super::VaultClientAdapter;

    /// adapter の default 構築が runtime 状態や外部接続を開始しないことを確認する。
    #[test]
    fn adapter_constructs_with_default() {
        let _ = VaultClientAdapter;
    }
}

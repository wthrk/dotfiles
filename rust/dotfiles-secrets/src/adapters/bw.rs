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
use bitwarden_api_api::models::{
    CipherCreateRequestModel, CipherRequestModel, CipherSecureNoteModel, SecureNoteType,
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden_crypto::{KeyDecryptable, KeyEncryptable, SymmetricCryptoKey};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden_vault::{
    Cipher, CipherRepromptType, CipherType, CipherView, ClientVaultExt, SyncRequest,
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use uuid::Uuid;

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::{
    Result,
    domain::{
        gpg_backup::GpgBackupEnvelope,
        pass_restore::PasswordStoreRemote,
        vault::{BitwardenVaultCredentials, VaultLookupCandidate, VaultSecretId, VaultSecretName},
    },
    ports::bw::VaultClientPort,
    support::protection::bitwarden_account_api,
};

/// Bitwarden 個人 vault SDK/API を `VaultClientPort` へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct VaultClientAdapter;

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
        if item.value.is_empty() {
            return Err(
                missing_data("Bitwarden personal vault secure note value is missing").into(),
            );
        }
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
        if item.value.is_empty() {
            return Err(
                missing_data("Bitwarden personal vault secure note value is missing").into(),
            );
        }
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
        Box::pin(async move { load_personal_vault_item(credentials, id).await })
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
        .filter(|cipher| cipher.deleted_date.is_none() && cipher.organization_id.is_none())
        .map(|cipher| personal_vault_item_from_cipher(cipher, &user_key))
        .filter_map(filter_secure_note_item)
        .collect()
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
async fn load_personal_vault_item(
    credentials: &BitwardenVaultCredentials,
    id: &str,
) -> Result<PersonalVaultItem> {
    let client = bitwarden_account_api::authenticate_with_account_api_key(
        credentials.api_key().client_id(),
        credentials.api_key().client_secret(),
        credentials.master_password(),
    )
    .await?;
    let user_key = SymmetricCryptoKey::try_from(client.crypto().get_user_encryption_key().await?)
        .context("Bitwarden personal vault user key could not be loaded")?;
    let id = id
        .parse::<Uuid>()
        .context("Bitwarden personal vault item ID is not a valid UUID")?;
    let configuration = client.internal.get_api_configurations().await;
    let cipher = ciphers_api::ciphers_id_details_get(&configuration.api, id)
        .await
        .context("Bitwarden personal vault item fetch failed")?;
    let cipher: Cipher = cipher
        .try_into()
        .context("Bitwarden personal vault item response parse failed")?;
    if cipher.deleted_date.is_some() {
        anyhow::bail!("Bitwarden personal vault item not found");
    }
    match personal_vault_item_from_cipher(cipher, &user_key)? {
        Some(item) => Ok(item),
        None => Err(missing_data("Bitwarden personal vault item is not a secure note").into()),
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn personal_vault_item_from_cipher(
    cipher: Cipher,
    user_key: &SymmetricCryptoKey,
) -> Result<Option<PersonalVaultItem>> {
    if !matches!(cipher.r#type, CipherType::SecureNote) {
        return Ok(None);
    }
    let view: CipherView = cipher
        .decrypt_with_key(user_key)
        .context("Bitwarden personal vault item decrypt failed")?;
    if view.name.is_empty() {
        return Err(missing_data("Bitwarden personal vault secure note name is missing").into());
    }
    if cipher.notes.is_some() && view.notes.is_none() {
        anyhow::bail!("Bitwarden personal vault secure note value decrypt failed");
    }
    Ok(Some(PersonalVaultItem {
        id: view
            .id
            .map(|id| id.to_string())
            .ok_or_else(|| missing_data("Bitwarden personal vault item ID is missing"))?,
        name: view.name,
        value: view.notes.unwrap_or_default(),
    }))
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn filter_secure_note_item(
    item: Result<Option<PersonalVaultItem>>,
) -> Option<Result<PersonalVaultItem>> {
    match item {
        Ok(Some(item)) => Some(Ok(item)),
        Ok(None) => None,
        Err(err) => Some(Err(err)),
    }
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
    let mut request: CipherRequestModel = serde_json::from_value(serde_json::to_value(encrypted)?)
        .context("Bitwarden personal vault item request encode failed")?;
    request.secure_note = Some(Box::new(CipherSecureNoteModel {
        r#type: Some(SecureNoteType::Generic),
    }));
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

#[cfg(all(test, not(feature = "secrets-internal-test-stub")))]
/// Bitwarden 個人 vault adapter の inline unit test。
mod tests {
    use bitwarden_crypto::{KeyEncryptable, SymmetricCryptoKey};
    use bitwarden_vault::{Cipher, CipherRepromptType, CipherType, CipherView};

    use super::{VaultClientAdapter, personal_vault_item_from_cipher};

    fn sample_user_key() -> SymmetricCryptoKey {
        match "w2LO+nwV4oxwswVYCxlOfRUseXfvU03VzvKQHrqeklPgiMZrspUe6sOBToCnDn9Ay0tuCBn8ykVVRb7PWhub2Q=="
            .to_string()
            .try_into()
        {
            Ok(key) => key,
            Err(_) => panic!("sample user key constant must be a valid SymmetricCryptoKey"),
        }
    }

    fn alternate_user_key() -> SymmetricCryptoKey {
        match "u4LO+nwV4oxwswVYCxlOfRUseXfvU03VzvKQHrqeklPgiMZrspUe6sOBToCnDn9Ay0tuCBn8ykVVRb7PWhub2Q=="
            .to_string()
            .try_into()
        {
            Ok(key) => key,
            Err(_) => panic!("alternate user key constant must be a valid SymmetricCryptoKey"),
        }
    }

    fn secure_note_view(name: &str, value: &str) -> CipherView {
        let now = chrono::Utc::now();
        CipherView {
            id: Some(uuid::Uuid::new_v4()),
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
        }
    }

    /// adapter の default 構築が runtime 状態や外部接続を開始しないことを確認する。
    #[test]
    fn adapter_constructs_with_default() {
        let _ = VaultClientAdapter;
    }

    #[test]
    fn personal_vault_item_ignores_non_secure_note() {
        let key = sample_user_key();
        let mut view = secure_note_view("name", "value");
        view.r#type = CipherType::Login;
        let cipher = view.encrypt_with_key(&key).expect("encrypt cipher");

        let item = personal_vault_item_from_cipher(cipher, &key).expect("convert item");

        assert!(item.is_none());
    }

    #[test]
    fn personal_vault_item_returns_secure_note_value() {
        let key = sample_user_key();
        let cipher = secure_note_view("backup", "{\"k\":\"v\"}")
            .encrypt_with_key(&key)
            .expect("encrypt cipher");

        let item = personal_vault_item_from_cipher(cipher, &key)
            .expect("convert item")
            .expect("secure note item");

        assert_eq!(item.name, "backup");
        assert_eq!(item.value, "{\"k\":\"v\"}");
    }

    #[test]
    fn personal_vault_item_rejects_note_decrypt_failure() {
        let key = sample_user_key();
        let alternate_key = alternate_user_key();
        let mut cipher: Cipher = secure_note_view("backup", "{\"k\":\"v\"}")
            .encrypt_with_key(&key)
            .expect("encrypt cipher");
        cipher.notes = secure_note_view("backup", "{\"k\":\"v2\"}")
            .encrypt_with_key(&alternate_key)
            .expect("encrypt cipher with alternate key")
            .notes;

        let error: anyhow::Error = match personal_vault_item_from_cipher(cipher, &key) {
            Ok(_) => panic!("decrypt should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("Bitwarden personal vault secure note value decrypt failed")
        );
    }

    #[test]
    fn personal_vault_item_keeps_personal_cipher_data_when_organization_is_absent() {
        let key = sample_user_key();
        let cipher = secure_note_view("backup", "{\"k\":\"v\"}")
            .encrypt_with_key(&key)
            .expect("encrypt cipher");

        let item = personal_vault_item_from_cipher(cipher, &key)
            .expect("convert item")
            .expect("secure note item");

        assert_eq!(item.name, "backup");
    }
}

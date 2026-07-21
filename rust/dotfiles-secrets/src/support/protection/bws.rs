//! BWS SDK が要求する所有 plaintext buffer と secret 返却値を protection 境界内で扱う操作。
//!
//! ## 出典と適用判断
//!
//! repository 正本は [`secret-recovery-spec.md` の「Bitwarden Secrets Manager」](../../../../../docs/secret-recovery/secret-recovery-spec.md#bitwarden-secrets-manager)、
//! [`bitwarden-personal-vault-design.md`](../../../../../docs/secret-recovery/bitwarden-personal-vault-design.md)、
//! [`secret-handling.md`](../../../../../docs/secret-recovery/secret-handling.md) である。YubiKey 保存の
//! `bitwarden-client-secret` だけで BWS を認証し、返却 value と request の所有文字列をこの
//! protection 境界から外へ平文化しない。
//!
//! vendor 全体 flow / 権限境界は [Bitwarden Developer Quick Start](https://bitwarden.com/help/developer-quick-start/)、
//! [Secrets Manager SDK](https://bitwarden.com/help/secrets-manager-sdk/)、
//! [Machine Accounts](https://bitwarden.com/help/machine-accounts/) を直接確認する。固定 SDK source は
//! `bitwarden` 2.1.0 [`Client::new` / `AccessTokenLoginRequest` / `AuthClient::login_access_token` sample](https://docs.rs/crate/bitwarden/2.1.0/source/src/lib.rs)、
//! `bitwarden-sm` 3.0.0 [`SecretsClient::get` / `create` / `update`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/client_secrets.rs)、
//! [`SecretCreateRequest`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/secrets/create.rs) /
//! [`SecretPutRequest`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/secrets/update.rs)、
//! [`SecretResponse::project_id`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/secrets/secret_response.rs)、
//! [`SecretsManagerError`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/error.rs) である。
//!
//! `login_access_token` 完了後だけ get/create/update を呼ぶ。これらの `Result<_, SecretsManagerError>`
//! の全 variant は context を加えても source chain を保持して failure として返し、not-found、権限、
//! transient、空結果、success へ再分類しない。`get` の `SecretResponse` は value/key/note を
//! `Zeroizing` / `ProtectedSecret` 内で消費する。fixed source の `SecretResponse::project_id` は
//! `Option<Uuid>` なので `None` を caller project で補完せず failure にする。create/update は caller が
//! 既に決めた organization / project / key を request に渡す技術操作だけを行う。project の一意解決や
//! fallback はこの module の責務ではない。
#![cfg_attr(feature = "secrets-internal-test-stub", allow(dead_code))]

use anyhow::Context;
use bitwarden::{
    Client, ClientSettings, DeviceType,
    auth::login::AccessTokenLoginRequest,
    secrets_manager::secrets::{SecretCreateRequest, SecretGetRequest, SecretPutRequest},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use super::SecretSession;
use crate::{
    Result,
    domain::{
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
        pass_restore::PasswordStoreRemote,
    },
    support::protection::ProtectedSecret,
};

/// BWS response value を protection 借用内で domain parser に渡す。
///
/// parser の選択と返却 domain 値の意味づけは application/domain の責務である。この操作は SDK が
/// 返した `String` を平文のまま port/application へ渡さず、UTF-8 validation と parser 呼び出しを
/// protected borrow 内で終える技術境界だけを提供する。
pub(crate) fn parse_response_value<T>(
    value: &ProtectedSecret,
    parse: impl FnOnce(&str) -> Result<T>,
) -> Result<T> {
    value.with_secret(|bytes| {
        let text =
            std::str::from_utf8(bytes).context("Bitwarden SDK secret value is not valid UTF-8")?;
        parse(text)
    })
}

fn protect_response_string(value: Zeroizing<String>) -> Result<ProtectedSecret> {
    let mut protected = ProtectedSecret::new(value.len())?;
    protected.with_secret_mut(|out| out.copy_from_slice(value.as_bytes()));
    Ok(protected)
}

/// SDK login request の repository 所有 access token buffer を Drop で zeroize する guard。
///
/// caller はこの guard から借用した request を SDK 呼び出し境界内だけで使う。future が
/// panic/unwind した場合でも guard の Drop が走る限り `access_token` buffer を zeroize する。
struct ZeroizingAccessTokenLoginRequest {
    inner: AccessTokenLoginRequest,
}

/// SDK update request が借用を終えるまで repository 所有の文字列を zeroize する guard。
///
/// `SecretPutRequest` は SDK 呼び出し中も `note` / `value` を所有する。SDK へ借用で渡す間は
/// この guard が request を保持し、成功・SDK error・guard 一致失敗後の early return のいずれでも
/// repository が所有する response 由来の `note` を Drop 時に zeroize する。
struct ZeroizingSecretPutRequest {
    inner: SecretPutRequest,
}

/// SDK create request が所有する `key` / `value` / `note` を呼び出し完了後に zeroize する guard。
struct ZeroizingSecretCreateRequest {
    inner: SecretCreateRequest,
}

/// core dump 抑止を確立した BWS SDK client session。
///
/// BWS SDK の login/list/get 境界へ入る前に `SecretSession` を開始し、access token と返却 secret
/// value を扱う SDK 呼び出し期間中は process-wide core dump 抑止済みであることを型で保持する。
pub(crate) struct BwsClientSession {
    client: Client,
    _session: SecretSession,
}

impl ZeroizingAccessTokenLoginRequest {
    fn new(access_token: String) -> Self {
        Self {
            inner: AccessTokenLoginRequest {
                access_token,
                state_file: None,
            },
        }
    }

    fn as_request(&self) -> &AccessTokenLoginRequest {
        &self.inner
    }
}

impl Drop for ZeroizingAccessTokenLoginRequest {
    fn drop(&mut self) {
        self.inner.access_token.zeroize();
    }
}

impl ZeroizingSecretPutRequest {
    fn new(inner: SecretPutRequest) -> Self {
        Self { inner }
    }

    fn as_request(&self) -> &SecretPutRequest {
        &self.inner
    }
}

impl ZeroizingSecretCreateRequest {
    fn new(inner: SecretCreateRequest) -> Self {
        Self { inner }
    }

    fn as_request(&self) -> &SecretCreateRequest {
        &self.inner
    }
}

impl Drop for ZeroizingSecretPutRequest {
    fn drop(&mut self) {
        self.inner.key.zeroize();
        self.inner.value.zeroize();
        self.inner.note.zeroize();
    }
}

impl Drop for ZeroizingSecretCreateRequest {
    fn drop(&mut self) {
        self.inner.key.zeroize();
        self.inner.value.zeroize();
        self.inner.note.zeroize();
    }
}

impl BwsClientSession {
    /// SDK client を借用し、BWS list/get 呼び出しを session lifetime 内で実行させる。
    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    /// SDK get で取得した secret value を protected buffer へ移し、revision と同じ borrow 境界で処理する。
    ///
    /// 返却 secret の plaintext はこの protection backend 操作内に閉じ、caller へ `ProtectedSecret` の
    /// 汎用 borrow API を渡さない。caller は closure で検証済み value または error だけを返す。
    pub(crate) async fn read_secret_value_with_revision(
        &self,
        id: Uuid,
    ) -> Result<(ProtectedSecret, BackupUpdateGuard)> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get failed")?;
        let _key = Zeroizing::new(secret.key);
        let _note = Zeroizing::new(secret.note);
        let revision = secret.revision_date.to_rfc3339();
        let value = Zeroizing::new(secret.value);
        let guard = BackupUpdateGuard::from_revision_or_value(revision, value.as_bytes());
        Ok((protect_response_string(value)?, guard))
    }

    /// SDK get の raw value を protection 内で借用し、検証済みの境界型だけを返す。
    pub(crate) async fn read_secret_value(&self, id: Uuid) -> Result<ProtectedSecret> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get failed")?;
        let _key = Zeroizing::new(secret.key);
        let _note = Zeroizing::new(secret.note);
        let value = Zeroizing::new(secret.value);
        protect_response_string(value)
    }

    /// current value を外へ出さずに stale-overwrite guard を作る。
    pub(crate) async fn current_secret_guard(&self, id: Uuid) -> Result<BackupUpdateGuard> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get for update guard failed")?;
        let _key = Zeroizing::new(secret.key);
        let _note = Zeroizing::new(secret.note);
        let value = Zeroizing::new(secret.value);
        Ok(BackupUpdateGuard::from_revision_or_value(
            secret.revision_date.to_rfc3339(),
            value.as_bytes(),
        ))
    }

    /// raw value/note/project assignment をこの protection 境界に閉じた guarded update。
    pub(crate) async fn update_secret_if_unchanged(
        &self,
        id: Uuid,
        expected_project_id: Uuid,
        key: String,
        value: Zeroizing<String>,
        expected_guard: &BackupUpdateGuard,
    ) -> Result<()> {
        let current = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get before guarded update failed")?;
        let current_key = Zeroizing::new(current.key);
        let current_value = Zeroizing::new(current.value);
        let current_note = Zeroizing::new(current.note);
        let current_guard = BackupUpdateGuard::from_revision_or_value(
            current.revision_date.to_rfc3339(),
            current_value.as_bytes(),
        );
        let current_project_id = guarded_update_preflight(
            &current_key,
            &current_note,
            expected_guard,
            &current_guard,
            current.project_id,
            expected_project_id,
        )?;
        let request = ZeroizingSecretPutRequest::new(SecretPutRequest {
            id,
            organization_id: current.organization_id,
            key,
            value: value.to_string(),
            note: current_note.to_string(),
            project_ids: Some(vec![current_project_id]),
        });
        self.client
            .secrets()
            .update(request.as_request())
            .await
            .context("Bitwarden SDK guarded secret update failed")?;
        Ok(())
    }

    pub(crate) async fn create_gpg_backup_envelope(
        &self,
        organization_id: Uuid,
        project_id: Uuid,
        key: &str,
        envelope: &GpgBackupEnvelope,
    ) -> Result<Uuid> {
        self.create_secret(
            organization_id,
            project_id,
            key,
            Zeroizing::new(envelope.to_json_string()?),
        )
        .await
    }

    pub(crate) async fn create_password_store_remote(
        &self,
        organization_id: Uuid,
        project_id: Uuid,
        key: &str,
        remote: &PasswordStoreRemote,
    ) -> Result<Uuid> {
        self.create_secret(
            organization_id,
            project_id,
            key,
            Zeroizing::new(remote.as_str().to_owned()),
        )
        .await
    }

    async fn create_secret(
        &self,
        organization_id: Uuid,
        project_id: Uuid,
        key: &str,
        value: Zeroizing<String>,
    ) -> Result<Uuid> {
        let request = ZeroizingSecretCreateRequest::new(SecretCreateRequest {
            organization_id,
            key: key.to_owned(),
            value: value.to_string(),
            note: String::new(),
            project_ids: Some(vec![project_id]),
        });
        let mut created = self
            .client
            .secrets()
            .create(request.as_request())
            .await
            .context("Bitwarden SDK secret create failed")?;
        let id = created.id;
        created.key.zeroize();
        created.value.zeroize();
        created.note.zeroize();
        Ok(id)
    }

    pub(crate) async fn update_gpg_backup_envelope_if_unchanged(
        &self,
        id: Uuid,
        expected_project_id: Uuid,
        key: String,
        envelope: &GpgBackupEnvelope,
        expected_guard: &BackupUpdateGuard,
    ) -> Result<()> {
        self.update_secret_if_unchanged(
            id,
            expected_project_id,
            key,
            Zeroizing::new(envelope.to_json_string()?),
            expected_guard,
        )
        .await
    }

    pub(crate) async fn update_password_store_remote_if_unchanged(
        &self,
        id: Uuid,
        expected_project_id: Uuid,
        key: String,
        remote: &PasswordStoreRemote,
        expected_guard: &BackupUpdateGuard,
    ) -> Result<()> {
        self.update_secret_if_unchanged(
            id,
            expected_project_id,
            key,
            Zeroizing::new(remote.as_str().to_owned()),
            expected_guard,
        )
        .await
    }
}

/// guarded update の停止条件を、response 由来 note の protection lifetime 内で確認する。
///
/// 固定 `bitwarden-sm` 3.0.0 source の
/// [`SecretResponse::project_id`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/secrets/secret_response.rs)
/// は `Option<Uuid>` である。したがって `None` は caller の `expected_project_id` で補わず failure
/// にする。`note` はこの関数の error return（guard 不一致、project 欠落・変更）まで `Zeroizing` のまま
/// 保持される。caller は成功時だけ SDK request へ note を複製し、request guard が SDK update 完了まで
/// その複製を zeroize 対象として保持する。
fn guarded_update_preflight(
    key: &Zeroizing<String>,
    note: &Zeroizing<String>,
    expected_guard: &BackupUpdateGuard,
    current_guard: &BackupUpdateGuard,
    current_project_id: Option<Uuid>,
    expected_project_id: Uuid,
) -> Result<Uuid> {
    let _protected_key_len = key.len();
    let _protected_note_len = note.len();
    expected_guard.ensure_matches(current_guard)?;
    let current_project_id = current_project_id
        .ok_or_else(|| anyhow::anyhow!("Bitwarden SDK secret response omitted project_id"))?;
    if current_project_id != expected_project_id {
        anyhow::bail!("Bitwarden SDK secret project changed before guarded update");
    }
    Ok(current_project_id)
}

#[cfg_attr(
    test,
    expect(
        clippy::items_after_test_module,
        reason = "login helper follows its guarded-update unit tests so the test-only response fixture remains adjacent to the preflight boundary"
    )
)]
#[cfg(test)]
mod tests {
    use super::guarded_update_preflight;
    use crate::domain::gpg_backup::BackupUpdateGuard;
    use uuid::Uuid;
    use zeroize::Zeroizing;

    #[test]
    fn guarded_update_preflight_keeps_response_note_protected_on_guard_failure() {
        let note = Zeroizing::new("response note".to_owned());
        let result = guarded_update_preflight(
            &Zeroizing::new("response key".to_owned()),
            &note,
            &BackupUpdateGuard::ValueDigest("expected".to_owned()),
            &BackupUpdateGuard::ValueDigest("current".to_owned()),
            Some(Uuid::new_v4()),
            Uuid::new_v4(),
        );
        assert!(result.is_err());
        assert_eq!(note.as_str(), "response note");
    }

    #[test]
    fn guarded_update_preflight_keeps_response_note_protected_on_project_failure() {
        let note = Zeroizing::new("response note".to_owned());
        let project = Uuid::new_v4();
        let result = guarded_update_preflight(
            &Zeroizing::new("response key".to_owned()),
            &note,
            &BackupUpdateGuard::ValueDigest("same".to_owned()),
            &BackupUpdateGuard::ValueDigest("same".to_owned()),
            Some(project),
            Uuid::new_v4(),
        );
        assert!(result.is_err());
        assert_eq!(note.as_str(), "response note");
    }
}

/// BWS access token で SDK 認証済み client を作成する。
///
/// ここでは SDK が要求する所有 token buffer の作成、login 呼び出し、repository 所有 buffer の
/// zeroize だけを完了する。project/secret lookup rule や外部確認 plan は扱わない。
///
/// 一次資料: `Client::new` / `AuthClient::login_access_token` の SDK flow と、machine-account
/// access token が許可された project だけを操作できる権限境界は
/// [`external-sdk-evidence.md`](../../../../../docs/secret-recovery/external-sdk-evidence.md#bitwarden-secrets-manager-sdk)
/// を参照する。SDK error は意味を再分類せず、source chain を保った context 付き failure として返す。
pub(crate) async fn login_client_with_access_token(
    access_token: &ProtectedSecret,
) -> Result<BwsClientSession> {
    let session = SecretSession::start()?;
    let client = access_token
        .with_secret_utf8_async(|token| {
            Box::pin(async {
                let settings = ClientSettings {
                    identity_url: "https://identity.bitwarden.eu".to_string(),
                    api_url: "https://api.bitwarden.eu".to_string(),
                    user_agent: "Bitwarden Rust-SDK".to_string(),
                    device_type: DeviceType::SDK,
                    device_identifier: None,
                    bitwarden_package_type: None,
                    bitwarden_client_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                };
                let client = Client::new(Some(settings));
                // Access-token bytes are credential material. Bitwarden の公開 sample / request API は trim や
                // whitespace normalization を定義していないため、YubiKey から復号した値を変更せずに渡す。
                // 空白除去で別 token を捏造したり、invalid token を成功扱いしたりしない。
                let request = ZeroizingAccessTokenLoginRequest::new(token.to_owned());
                client
                    .auth()
                    .login_access_token(request.as_request())
                    .await
                    .context("Bitwarden SDK access-token login failed")?;
                drop(request);
                Ok(client)
            })
        })
        .await?;
    Ok(BwsClientSession {
        client,
        _session: session,
    })
}

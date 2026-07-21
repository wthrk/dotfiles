//! BWS SDK が要求する所有 plaintext buffer と secret 返却値を protection 境界内で扱う操作。
#![cfg_attr(feature = "secrets-internal-test-stub", allow(dead_code))]

use anyhow::Context;
use bitwarden::{
    Client, ClientSettings, DeviceType,
    auth::login::AccessTokenLoginRequest,
    secrets_manager::secrets::{SecretGetRequest, SecretPutRequest},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use super::SecretSession;
use crate::{Result, domain::gpg_backup::BackupUpdateGuard, support::protection::ProtectedSecret};

/// SDK login request の repository 所有 access token buffer を Drop で zeroize する guard。
///
/// caller はこの guard から借用した request を SDK 呼び出し境界内だけで使う。future が
/// panic/unwind した場合でも guard の Drop が走る限り `access_token` buffer を zeroize する。
struct ZeroizingAccessTokenLoginRequest {
    inner: AccessTokenLoginRequest,
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

impl BwsClientSession {
    /// SDK client を借用し、BWS list/get 呼び出しを session lifetime 内で実行させる。
    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    /// SDK get で取得した secret value を protected buffer へ移し、revision と同じ borrow 境界で処理する。
    ///
    /// 返却 secret の plaintext はこの protection backend 操作内に閉じ、caller へ `ProtectedSecret` の
    /// 汎用 borrow API を渡さない。caller は closure で検証済み value または error だけを返す。
    pub(crate) async fn parse_secret_value_with_revision<R>(
        &self,
        id: Uuid,
        parse: impl FnOnce(&str, &str) -> Result<R>,
    ) -> Result<R> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get failed")?;
        let revision = secret.revision_date.to_rfc3339();
        let value = Zeroizing::new(secret.value);
        let mut protected = ProtectedSecret::new(value.len())?;
        protected.with_secret_mut(|out| out.copy_from_slice(value.as_bytes()));
        protected.with_secret_utf8(|json| parse(json, revision.as_str()))
    }

    /// SDK get の raw value を protection 内で借用し、検証済みの境界型だけを返す。
    pub(crate) async fn parse_secret_value<R>(
        &self,
        id: Uuid,
        parse: impl FnOnce(&str) -> Result<R>,
    ) -> Result<R> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get failed")?;
        let value = Zeroizing::new(secret.value);
        parse(value.as_str())
    }

    /// current value を外へ出さずに stale-overwrite guard を作る。
    pub(crate) async fn current_secret_guard(&self, id: Uuid) -> Result<BackupUpdateGuard> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get for update guard failed")?;
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
        value: String,
        expected_guard: &BackupUpdateGuard,
    ) -> Result<()> {
        let current = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get before guarded update failed")?;
        let current_value = Zeroizing::new(current.value);
        let current_guard = BackupUpdateGuard::from_revision_or_value(
            current.revision_date.to_rfc3339(),
            current_value.as_bytes(),
        );
        expected_guard.ensure_matches(&current_guard)?;
        let current_project_id = current
            .project_id
            .ok_or_else(|| anyhow::anyhow!("Bitwarden SDK secret response omitted project_id"))?;
        if current_project_id != expected_project_id {
            anyhow::bail!("Bitwarden SDK secret project changed before guarded update");
        }
        self.client
            .secrets()
            .update(&SecretPutRequest {
                id,
                organization_id: current.organization_id,
                key,
                value,
                note: current.note,
                project_ids: Some(vec![current_project_id]),
            })
            .await
            .context("Bitwarden SDK guarded secret update failed")?;
        Ok(())
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

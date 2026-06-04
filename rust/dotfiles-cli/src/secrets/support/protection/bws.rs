//! BWS SDK が要求する所有 plaintext buffer と secret 返却値を protection 境界内で扱う操作。
#![cfg_attr(feature = "secrets-internal-test-stub", allow(dead_code))]

use bitwarden::{
    Client, auth::login::AccessTokenLoginRequest, secrets_manager::secrets::SecretGetRequest,
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use super::SecretSession;
use crate::{Result, secrets::support::protection::ProtectedSecret};

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
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        let revision = secret.revision_date.to_rfc3339();
        let value = Zeroizing::new(secret.value);
        let mut protected = ProtectedSecret::new(value.len())?;
        protected.with_secret_mut(|out| out.copy_from_slice(value.as_bytes()));
        protected.with_secret_utf8(|json| parse(json, revision.as_str()))
    }

    /// SDK get で取得した secret value を protected buffer 外へ出せる `Zeroizing` 管理の文字列として返す。
    ///
    /// caller は対象値が protected borrow 境界を必要としないことを protection 外で確定してから呼び出す。
    /// この support 境界は BWS SDK の secret value を repository 所有文字列へ移し、Drop 時の zeroize を
    /// 保証するだけで、保存モデル上の secret 名・用途判断は持たない。
    pub(crate) async fn get_non_secret_value(&self, id: Uuid) -> Result<Zeroizing<String>> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        Ok(Zeroizing::new(secret.value))
    }
}

/// BWS access token で SDK 認証済み client を作成する。
///
/// ここでは SDK が要求する所有 token buffer の作成、login 呼び出し、repository 所有 buffer の
/// zeroize だけを完了する。project/secret lookup rule や外部確認 plan は扱わない。
pub(crate) async fn login_client_with_access_token(
    access_token: &ProtectedSecret,
) -> Result<BwsClientSession> {
    let session = SecretSession::start()?;
    let client = access_token
        .with_secret_utf8_async(|token| {
            Box::pin(async {
                let client = Client::new(None);
                let request = ZeroizingAccessTokenLoginRequest::new(token.trim().to_owned());
                client
                    .auth()
                    .login_access_token(request.as_request())
                    .await
                    .map_err(|_| anyhow::anyhow!("bitwarden login failed"))?;
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

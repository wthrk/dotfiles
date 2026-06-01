//! BWS SDK が要求する所有 plaintext buffer と secret 返却値を protection 境界内で扱う操作。
#![cfg_attr(feature = "secrets-internal-test-stub", allow(dead_code))]

use bitwarden::{
    Client, auth::login::AccessTokenLoginRequest, secrets_manager::secrets::SecretGetRequest,
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use super::SecretSession;
use crate::{
    Result,
    secrets::{domain::pass_restore::PasswordStoreRemote, support::protection::ProtectedSecret},
};

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

    /// SDK get と返却 secret value の保護所有値化を、同じ session lifetime 内で完了する。
    ///
    /// caller は domain/application 側で一意解決済みの secret ID だけを渡す。この method は
    /// lookup rule や 0件/複数件の failure 化を扱わず、SDK get 呼び出しと保護境界だけを担う。
    pub(crate) async fn get_protected_secret_value(&self, id: Uuid) -> Result<ProtectedSecret> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        protect_secret_value(secret.value)
    }

    /// SDK get で取得した `password-store-remote` secret value を、protection 境界内で domain 値へ翻訳する。
    ///
    /// clone URL は秘密情報ではないが、SDK 返却 value は一旦 `Zeroizing` 管理へ入れてから
    /// [`PasswordStoreRemote::parse`] で検証する。検証済みの URL 文字列だけを domain 値として返し、
    /// SDK 返却 buffer は Drop で zeroize する。URL 形式の妥当性判断は domain rule に委ねる。
    pub(crate) async fn get_password_store_remote(&self, id: Uuid) -> Result<PasswordStoreRemote> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        let value = Zeroizing::new(secret.value);
        PasswordStoreRemote::parse(value.as_str())
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

/// BWS SDK から返った secret value を直ちに zeroize 管理へ移してから protection 所有値へ移す。
fn protect_secret_value(value: String) -> Result<ProtectedSecret> {
    let value = Zeroizing::new(value);
    let mut secret = ProtectedSecret::new(value.len())?;
    secret.with_secret_mut(|out| out.copy_from_slice(value.as_bytes()));
    Ok(secret)
}

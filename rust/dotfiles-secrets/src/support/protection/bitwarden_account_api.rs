//! Bitwarden account API key を SDK 認証呼び出しへ渡す secret 保護境界。
//!
//! この module は `client_id` / `client_secret` の平文借用と SDK 所有 `String` への移譲を
//! 同じ protection 境界内で完了する。application / adapter へ平文 buffer を返さない。

use anyhow::Context;
use bitwarden_core::{Client, auth::login::ApiKeyLoginRequest};
use zeroize::{Zeroize, Zeroizing};

use crate::Result;

use super::ProtectedSecret;

/// Bitwarden account API key で SDK client を認証する。
///
/// upstream SDK の API key 認証は account API key に加えて master password から user crypto を
/// 初期化する。master password は YubiKey へ保存せず、caller が CLI/app input port で取得した
/// `ProtectedSecret` をこの protection 境界へ渡す。
pub(crate) async fn authenticate_with_account_api_key(
    client_id: &ProtectedSecret,
    client_secret: &ProtectedSecret,
    master_password: &ProtectedSecret,
) -> Result<Client> {
    let client = Client::new(None);
    let client_id = protected_secret_to_zeroizing_string(client_id)?;
    let client_secret = protected_secret_to_zeroizing_string(client_secret)?;
    let master_password = protected_secret_to_zeroizing_string(master_password)?;
    let request = ZeroizingApiKeyLoginRequest::new(
        client_id.to_string(),
        client_secret.to_string(),
        master_password.to_string(),
    );
    let response = client.auth().login_api_key(request.as_request()).await;
    let response = response.context("Bitwarden personal vault SDK/API authentication failed")?;
    if !response.authenticated {
        anyhow::bail!("Bitwarden personal vault SDK/API authentication was not accepted");
    }
    Ok(client)
}

struct ZeroizingApiKeyLoginRequest {
    inner: ApiKeyLoginRequest,
}

impl ZeroizingApiKeyLoginRequest {
    fn new(client_id: String, client_secret: String, password: String) -> Self {
        Self {
            inner: ApiKeyLoginRequest {
                client_id,
                client_secret,
                password,
            },
        }
    }

    fn as_request(&self) -> &ApiKeyLoginRequest {
        &self.inner
    }
}

impl Drop for ZeroizingApiKeyLoginRequest {
    fn drop(&mut self) {
        self.inner.client_id.zeroize();
        self.inner.client_secret.zeroize();
        self.inner.password.zeroize();
    }
}

fn protected_secret_to_zeroizing_string(secret: &ProtectedSecret) -> Result<Zeroizing<String>> {
    secret.with_secret(|bytes| {
        let text =
            std::str::from_utf8(bytes).context("Bitwarden credential contains invalid UTF-8")?;
        Ok(Zeroizing::new(text.to_owned()))
    })
}

//! BWS SDK login request を protection 境界内で扱う操作。

use std::{future::Future, pin::Pin};

use bitwarden::auth::login::AccessTokenLoginRequest;
use zeroize::{Zeroize, Zeroizing};

use crate::Result;

use super::{ProtectedSecret, SecretSession};

/// SDK login request の repository 所有 access token buffer を Drop で zeroize する guard。
///
/// caller はこの guard から借用した request を SDK 呼び出し境界内だけで使う。future が
/// panic/unwind した場合でも guard の Drop が走る限り `access_token` buffer を zeroize する。
struct ZeroizingAccessTokenLoginRequest {
    inner: AccessTokenLoginRequest,
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

/// BWS access token の所有 request buffer を作り、Drop 境界で zeroize する。
pub(crate) async fn with_access_token_login_request<R>(
    access_token: &ProtectedSecret,
    call: impl for<'a> FnOnce(
        &'a AccessTokenLoginRequest,
    ) -> Pin<Box<dyn Future<Output = Result<R>> + 'a>>,
) -> Result<R> {
    let request = access_token.with_secret_utf8(|token| {
        Ok(ZeroizingAccessTokenLoginRequest::new(
            token.trim().to_owned(),
        ))
    })?;
    call(request.as_request()).await
}

/// BWS SDK から返った secret value を直ちに zeroize 管理へ移してから protection 所有値へ移す。
pub(crate) fn protect_secret_value(value: String) -> Result<ProtectedSecret> {
    let value = Zeroizing::new(value);
    let _session = SecretSession::start()?;
    let mut secret = ProtectedSecret::new(value.len())?;
    secret.with_secret_mut(|out| out.copy_from_slice(value.as_bytes()));
    Ok(secret)
}

//! Bitwarden Secrets Manager クライアント adapter の stub。
//!
//! BWS SDK 統合が完了するまで fetch を試みず、呼び出し側へ未実装エラーを返す。

use crate::{
    Result,
    secrets::{
        domain::{material::SecretMaterial, values::BwsSecretName},
        ports::BwsClientPort,
    },
};

/// Bitwarden Secrets Manager との通信を担う adapter。
///
/// BWS SDK 統合完了まで全 fetch 呼び出しへ未実装エラーを返す stub 実装を保持する。
#[derive(Default)]
pub(super) struct BwsClientAdapter;

impl BwsClientPort for BwsClientAdapter {
    fn fetch_bws_secret(
        &mut self,
        _access_token: &SecretMaterial,
        name: BwsSecretName,
    ) -> Result<SecretMaterial> {
        anyhow::bail!(
            "BWS SDK is not yet integrated: cannot fetch {}",
            name.as_str()
        )
    }
}

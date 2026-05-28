//! 実環境の BWS CLI 呼び出し実装。

use std::process::Command;

use crate::{
    Result,
    secrets::{
        domain::{material::SecretMaterial, values::BwsSecretName},
        ports::BwsClientPort,
        support::protection::{ProtectedSecret, secret_consumer},
    },
};

#[derive(Default)]
pub(crate) struct BwsClientAdapter;

impl BwsClientPort for BwsClientAdapter {
    fn fetch_bws_secret(
        &self,
        access_token: &SecretMaterial,
        secret_name: BwsSecretName,
    ) -> Result<SecretMaterial> {
        let protected = access_token
            .as_backend::<ProtectedSecret>()
            .ok_or_else(|| anyhow::anyhow!("bws access token backend is not protected memory"))?;
        let key = match secret_name {
            BwsSecretName::GpgSecretKeyBackup => "gpg-secret-key-backup",
            BwsSecretName::PasswordStoreRemote => "password-store-remote",
        };
        secret_consumer::with_utf8_secret(protected, |token| {
            let secret_id = self.resolve_secret_id(token.trim(), key)?;
            let output = Command::new("bws")
                .args(["secret", "get", &secret_id, "--output", "json"])
                .env("BWS_ACCESS_TOKEN", token.trim())
                .output()
                .map_err(|error| anyhow::anyhow!("failed to invoke bws CLI: {error}"))?;
            if !output.status.success() {
                let status = output.status.code().map_or_else(
                    || "terminated by signal".to_string(),
                    |code| code.to_string(),
                );
                return Err(anyhow::anyhow!(
                    "bws external check failed for {key}/{secret_id} (exit status: {status})"
                ));
            }
            let payload: serde_json::Value = serde_json::from_slice(&output.stdout)
                .map_err(|error| anyhow::anyhow!("failed to decode bws secret JSON: {error}"))?;
            let value = payload
                .get("value")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("bws secret response does not contain value"))?;
            Ok(SecretMaterial::from_backend(
                value.as_bytes().to_vec(),
                |secret| secret.len(),
                |secret| Ok(secret.clone()),
            ))
        })
    }
}

impl BwsClientAdapter {
    fn resolve_secret_id(&self, token: &str, key: &str) -> Result<String> {
        let output = Command::new("bws")
            .args(["secret", "list", "--output", "json"])
            .env("BWS_ACCESS_TOKEN", token)
            .output()
            .map_err(|error| anyhow::anyhow!("failed to invoke bws secret list: {error}"))?;
        if !output.status.success() {
            let status = output.status.code().map_or_else(
                || "terminated by signal".to_string(),
                |code| code.to_string(),
            );
            return Err(anyhow::anyhow!(
                "bws secret list failed for key {key} (exit status: {status})"
            ));
        }
        let payload: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| anyhow::anyhow!("failed to decode bws secret list JSON: {error}"))?;
        payload
            .as_array()
            .and_then(|secrets| {
                secrets.iter().find_map(|secret| {
                    let candidate_key = secret.get("key").and_then(serde_json::Value::as_str)?;
                    let candidate_id = secret.get("id").and_then(serde_json::Value::as_str)?;
                    (candidate_key == key).then(|| candidate_id.to_string())
                })
            })
            .ok_or_else(|| anyhow::anyhow!("bws secret key not found: {key}"))
    }
}

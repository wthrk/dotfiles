//! 実環境の BWS CLI 呼び出し実装。

use std::process::{Command, Output};

use crate::{
    Result,
    secrets::{
        domain::{material::SecretMaterial, values::BwsSecretName},
        ports::BwsClientPort,
        support::protection::{ProtectedSecret, secret_consumer},
    },
};

const MAX_BWS_SECRET_FIELD_LEN: usize = 1024 * 1024;

#[derive(Default)]
pub(crate) struct BwsClientAdapter;

#[derive(serde::Deserialize)]
struct BwsSecretListEntry<'a> {
    id: &'a str,
    key: &'a str,
}

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
            let protected_output = protected_stdout(output)?;
            let mut fields = protected_output.decode_json_string_map(MAX_BWS_SECRET_FIELD_LEN)?;
            let value = fields
                .remove("value")
                .ok_or_else(|| anyhow::anyhow!("bws secret response does not contain value"))?;
            Ok(SecretMaterial::from_backend(
                value,
                ProtectedSecret::len,
                ProtectedSecret::try_clone,
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
        let protected_output = protected_stdout(output)?;
        secret_consumer::with_secret_bytes(&protected_output, |bytes| {
            let secrets: Vec<BwsSecretListEntry<'_>> =
                serde_json::from_slice(bytes).map_err(|error| {
                    anyhow::anyhow!("failed to decode bws secret list JSON: {error}")
                })?;
            secrets
                .into_iter()
                .find_map(|secret| (secret.key == key).then(|| secret.id.to_string()))
                .ok_or_else(|| anyhow::anyhow!("bws secret key not found: {key}"))
        })
    }
}

fn protected_stdout(output: Output) -> Result<ProtectedSecret> {
    ProtectedSecret::from_vec(output.stdout)
}

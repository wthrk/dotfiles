//! 実環境の BWS CLI 呼び出し実装。

use std::process::{Command, Stdio};

use crate::{
    Result,
    secrets::{
        domain::{material::SecretMaterial, values::BwsSecretName},
        ports::BwsClientPort,
        support::protection::{
            ProtectedSecret, SecretSession, buffer::ProtectedInputBuffer, secret_consumer,
        },
    },
};

const MAX_BWS_CLI_JSON_LEN: usize = 8 * 1024 * 1024;
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
            let protected_output = run_bws_json(
                token.trim(),
                ["secret", "get", &secret_id, "--output", "json"],
                &format!("bws external check failed for {key}/{secret_id}"),
            )?;
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
        let protected_output = run_bws_json(
            token,
            ["secret", "list", "--output", "json"],
            &format!("bws secret list failed for key {key}"),
        )?;
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

fn run_bws_json<const N: usize>(
    token: &str,
    args: [&str; N],
    failure_context: &str,
) -> Result<ProtectedSecret> {
    let session = SecretSession::start()?;
    let mut child = Command::new("bws")
        .args(args)
        .env("BWS_ACCESS_TOKEN", token)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| anyhow::anyhow!("failed to invoke bws CLI: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture bws stdout"))?;
    let buffer = ProtectedInputBuffer::read_from(
        stdout,
        MAX_BWS_CLI_JSON_LEN,
        "bws JSON response exceeds maximum length",
        &session,
    )?;
    let status = child
        .wait()
        .map_err(|error| anyhow::anyhow!("failed to wait for bws CLI: {error}"))?;
    if !status.success() {
        let status = status.code().map_or_else(
            || "terminated by signal".to_string(),
            |code| code.to_string(),
        );
        return Err(anyhow::anyhow!("{failure_context} (exit status: {status})"));
    }
    buffer.into_protected_secret(
        &session,
        MAX_BWS_CLI_JSON_LEN,
        "bws JSON response exceeds maximum length",
    )
}

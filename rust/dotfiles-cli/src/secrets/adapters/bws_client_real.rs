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
const BWS_PROJECT_NAME: &str = "dotfiles-secret-recovery";

#[derive(Default)]
pub(crate) struct BwsClientAdapter;

#[derive(serde::Deserialize)]
struct BwsSecretListEntry<'a> {
    id: &'a str,
    key: &'a str,
}

#[derive(serde::Deserialize)]
struct BwsProjectListEntry<'a> {
    id: &'a str,
    name: &'a str,
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
            let project_id = self.resolve_project_id(token.trim())?;
            let secret_id = self.resolve_secret_id(token.trim(), &project_id, key)?;
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
    fn resolve_project_id(&self, token: &str) -> Result<String> {
        let protected_output = run_bws_json(
            token,
            ["project", "list", "--output", "json"],
            "bws project list failed",
        )?;
        secret_consumer::with_secret_bytes(&protected_output, |bytes| {
            let projects: Vec<BwsProjectListEntry<'_>> =
                serde_json::from_slice(bytes).map_err(|error| {
                    anyhow::anyhow!("failed to decode bws project list JSON: {error}")
                })?;
            let mut matches = projects
                .into_iter()
                .filter(|project| project.name == BWS_PROJECT_NAME);
            let Some(project) = matches.next() else {
                return Err(anyhow::anyhow!("bws project not found: {BWS_PROJECT_NAME}"));
            };
            if matches.next().is_some() {
                return Err(anyhow::anyhow!(
                    "multiple bws projects matched: {BWS_PROJECT_NAME}"
                ));
            }
            Ok(project.id.to_string())
        })
    }

    fn resolve_secret_id(&self, token: &str, project_id: &str, key: &str) -> Result<String> {
        let protected_output = run_bws_json(
            token,
            ["secret", "list", project_id, "--output", "json"],
            &format!("bws secret list failed for project {project_id} and key {key}"),
        )?;
        secret_consumer::with_secret_bytes(&protected_output, |bytes| {
            let secrets: Vec<BwsSecretListEntry<'_>> =
                serde_json::from_slice(bytes).map_err(|error| {
                    anyhow::anyhow!("failed to decode bws secret list JSON: {error}")
                })?;
            let mut matches = secrets.into_iter().filter(|secret| secret.key == key);
            let Some(secret) = matches.next() else {
                return Err(anyhow::anyhow!(
                    "bws secret key not found in project {project_id}: {key}"
                ));
            };
            if matches.next().is_some() {
                return Err(anyhow::anyhow!(
                    "multiple bws secret keys matched in project {project_id}: {key}"
                ));
            }
            Ok(secret.id.to_string())
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

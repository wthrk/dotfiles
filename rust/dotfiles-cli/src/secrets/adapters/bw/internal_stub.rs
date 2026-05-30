//! `secrets-internal-test-stub` feature 専用の BWS adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で
//! real BWS SDK backend と差し替わる。integration test はこの module を import せず、同じ
//! `dotfiles` binary を実行する。
//!
//! この stub は BWS port の datastore 境界だけを受け持つ。初期 datastore は
//! `DOTFILES_SECRETS_BWS_STUB_DATASTORE_JSON` から読み、最終 datastore は
//! `DOTFILES_SECRETS_BWS_STUB_OUTPUT_PATH` へ JSON として書き出す。YubiKey port stub とは
//! state/schema/file を共有しない。

use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::Context;

use crate::secrets::{
    domain::bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
    ports::bw::BwsClientPort,
    support::protection::ProtectedSecret,
};

const BWS_STUB_DATASTORE_ENV: &str = "DOTFILES_SECRETS_BWS_STUB_DATASTORE_JSON";
const BWS_STUB_OUTPUT_ENV: &str = "DOTFILES_SECRETS_BWS_STUB_OUTPUT_PATH";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct BwsDatastore {
    projects: BTreeMap<String, String>,
    project_secrets: BTreeMap<String, BTreeMap<String, String>>,
    secret_values: BTreeMap<String, String>,
}

impl BwsClientPort for super::BwsClientAdapter {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        read_bws_projects(access_token)
    }

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        read_bws_secrets(access_token, project_id)
    }

    async fn fetch_bws_secret_by_id(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<ProtectedSecret> {
        read_bws_secret_by_id(access_token, secret_id)
    }
}

fn read_bws_projects(
    access_token: &ProtectedSecret,
) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
    with_datastore(|store| {
        ensure_access_token_matches_datastore(access_token, store)?;
        Ok(store
            .projects
            .iter()
            .map(|(project_id, project_name)| BwsLookupCandidate {
                id: BwsProjectId::new(project_id.clone()),
                name: project_name.clone(),
            })
            .collect())
    })
}

fn read_bws_secrets(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
    with_datastore(|store| {
        ensure_access_token_matches_datastore(access_token, store)?;
        let candidates = store
            .project_secrets
            .get(project_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("bitwarden project not found"))?;
        Ok(candidates
            .iter()
            .map(|(secret_id, secret_name)| BwsLookupCandidate {
                id: BwsSecretId::new(secret_id.clone()),
                name: secret_name.clone(),
            })
            .collect())
    })
}

fn read_bws_secret_by_id(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> crate::Result<ProtectedSecret> {
    with_datastore(|store| {
        ensure_access_token_matches_datastore(access_token, store)?;
        let value = store
            .secret_values
            .get(secret_id.as_str())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
        protected_secret_from_string(value)
    })
}

fn ensure_access_token_matches_datastore(
    access_token: &ProtectedSecret,
    store: &BwsDatastore,
) -> crate::Result<()> {
    let configured = store
        .secret_values
        .get("bws-secret-id-access-token")
        .ok_or_else(|| anyhow::anyhow!("bws access token stub secret is not configured"))?;
    if access_token.to_test_bytes() == configured.as_bytes() {
        Ok(())
    } else {
        anyhow::bail!("bitwarden login failed")
    }
}

fn protected_secret_from_string(value: String) -> crate::Result<ProtectedSecret> {
    let session = crate::secrets::support::protection::SecretSession::start()?;
    let buffer = crate::secrets::support::protection::buffer::ProtectedInputBuffer::read_line_from(
        std::io::Cursor::new(value.into_bytes()),
        16 * 1024,
        &session,
    )?;
    buffer
        .into_protected_secret_line(&session, 16 * 1024, "internal stub secret is too large")
        .map_err(Into::into)
}

fn with_datastore<T>(f: impl FnOnce(&mut BwsDatastore) -> crate::Result<T>) -> crate::Result<T> {
    let mut store = load_datastore()?;
    let out = f(&mut store)?;
    write_observed_datastore(&store)?;
    Ok(out)
}

fn load_datastore() -> crate::Result<BwsDatastore> {
    let path = output_path()?;
    if path.exists() {
        let body = fs::read(&path)?;
        return serde_json::from_slice(&body)
            .context("failed to decode observed BWS internal stub datastore JSON");
    }
    let body = std::env::var(BWS_STUB_DATASTORE_ENV)
        .context("BWS internal stub datastore JSON is not configured")?;
    serde_json::from_str(&body).context("failed to decode BWS internal stub datastore JSON")
}

fn write_observed_datastore(store: &BwsDatastore) -> crate::Result<()> {
    let path = output_path()?;
    let body = serde_json::to_vec_pretty(store)?;
    fs::write(path, body)?;
    Ok(())
}

fn output_path() -> crate::Result<PathBuf> {
    let path = std::env::var(BWS_STUB_OUTPUT_ENV)
        .context("BWS internal stub output path is not configured")?;
    Ok(PathBuf::from(path))
}

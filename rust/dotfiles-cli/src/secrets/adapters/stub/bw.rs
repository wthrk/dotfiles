//! `secrets-internal-test-stub` feature 専用の file-backed BWS client stub。
//!
//! production build には compile されない adapter 配下 backend stub であり、integration test は
//! この module を import せず feature 有効でビルドされた同じ `dotfiles` binary を実行する。
//! `DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH` の state file を backend として読み、real/stub の
//! 切替は runtime 分岐ではなく compile-time feature selection で行う。
//
// verify-yubikey --check bws の external check をネットワークへ接続せず再現し、
// BWS 側 state に保存された project/secret/value と fetch 監査イベントを共有 state へ記録する。

use std::{collections::BTreeMap, fs};

use anyhow::Context;

use crate::secrets::{
    domain::bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
    ports::BwsClientPort,
    support::protection::ProtectedSecret,
};

use super::BwsClientAdapter;

const INTERNAL_STUB_STATE_ENV: &str = "DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StubState {
    key_exists: std::collections::BTreeMap<u32, bool>,
    objects: std::collections::BTreeMap<(u32, u32), Vec<u8>>,
    plaintexts: std::collections::BTreeMap<(u32, u8), Vec<u8>>,
    corrupt: std::collections::BTreeSet<(u32, u8)>,
    include_spare: bool,
    requires_pin: bool,
    write_events: Vec<String>,
    #[serde(default)]
    bws_projects: BTreeMap<String, String>,
    #[serde(default)]
    bws_project_secrets: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    bws_secret_values: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    bws_fetch_events: Vec<String>,
}

impl BwsClientPort for BwsClientAdapter {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        list_bws_projects(access_token)
    }

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        list_bws_secrets(access_token, project_id)
    }

    async fn fetch_bws_secret_by_id(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<ProtectedSecret> {
        fetch_bws_secret_by_id(access_token, secret_id)
    }
}

fn list_bws_projects(
    access_token: &ProtectedSecret,
) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
    with_state(|state| {
        ensure_access_token_matches_state(access_token, state)?;
        let mut out = Vec::with_capacity(state.bws_projects.len());
        for (project_id, project_name) in &state.bws_projects {
            out.push(BwsLookupCandidate {
                id: BwsProjectId::new(project_id.clone()),
                name: project_name.clone(),
            });
        }
        Ok(out)
    })
}

fn list_bws_secrets(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
    with_state(|state| {
        ensure_access_token_matches_state(access_token, state)?;
        let candidates = state
            .bws_project_secrets
            .get(project_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("bitwarden project not found"))?;
        let mut out = Vec::with_capacity(candidates.len());
        for (secret_id, secret_name) in candidates {
            out.push(BwsLookupCandidate {
                id: BwsSecretId::new(secret_id.clone()),
                name: secret_name.clone(),
            });
        }
        Ok(out)
    })
}

fn fetch_bws_secret_by_id(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> crate::Result<ProtectedSecret> {
    with_state(|state| {
        ensure_access_token_matches_state(access_token, state)?;
        let bytes = state
            .bws_secret_values
            .get(secret_id.as_str())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
        state.bws_fetch_events.push(format!(
            "DOTFILES_TEST_BWS_FETCH id={} bytes={}",
            secret_id.as_str(),
            bytes.len()
        ));
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let buffer =
            crate::secrets::support::protection::buffer::ProtectedInputBuffer::read_line_from(
                std::io::Cursor::new(bytes),
                16 * 1024,
                &session,
            )?;
        buffer
            .into_protected_secret_line(&session, 16 * 1024, "internal stub secret is too large")
            .map_err(Into::into)
    })
}

fn ensure_access_token_matches_state(
    access_token: &ProtectedSecret,
    state: &StubState,
) -> crate::Result<()> {
    let configured = state
        .bws_secret_values
        .get("bws-secret-id-access-token")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("bws access token stub secret is not configured"))?;
    if access_token.to_test_bytes() == configured {
        Ok(())
    } else {
        anyhow::bail!("bitwarden login failed")
    }
}

fn with_state<T>(f: impl FnOnce(&mut StubState) -> crate::Result<T>) -> crate::Result<T> {
    let path = endpoint()?;
    let mut state = if path.exists() {
        let body = fs::read(&path)?;
        bincode::serde::decode_from_slice::<StubState, _>(&body, bincode::config::standard())
            .map(|(state, _)| state)
            .with_context(|| format!("failed to decode internal stub state: {}", path.display()))?
    } else {
        StubState::default()
    };
    let out = f(&mut state)?;
    let encoded = bincode::serde::encode_to_vec(&state, bincode::config::standard())?;
    fs::write(&path, encoded)?;
    Ok(out)
}

fn endpoint() -> crate::Result<std::path::PathBuf> {
    let path = std::env::var(INTERNAL_STUB_STATE_ENV)
        .context("internal stub state path is not configured")?;
    Ok(std::path::PathBuf::from(path))
}

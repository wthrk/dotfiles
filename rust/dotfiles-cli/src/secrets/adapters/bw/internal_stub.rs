//! `secrets-internal-test-stub` feature 専用の file-backed BWS adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で
//! real BWS SDK backend と差し替わる。integration test はこの module を import せず、同じ
//! `dotfiles` binary を実行し、fixture が作る `DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH` の
//! state file を backend として共有する。

use std::{collections::BTreeMap, fs};

use anyhow::Context;

use crate::secrets::{
    domain::bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
    ports::bw::BwsClientPort,
    support::protection::ProtectedSecret,
};

const INTERNAL_STUB_STATE_ENV: &str = "DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH";

#[derive(serde::Serialize, serde::Deserialize, Default)]
/// adapter stub が state file から読む最小 schema。
///
/// fixture builder や assertion helper は tests 側に残し、この型は backend が port 契約を
/// 再現するために必要な永続 state だけを持つ。
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

/// state file の project 一覧を `BwsClientPort` の lookup 候補へ翻訳する。
fn read_bws_projects(
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

/// state file の project secret 一覧を `BwsClientPort` の lookup 候補へ翻訳する。
fn read_bws_secrets(
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

/// state file の secret value を保護済み secret として返し、fetch 監査イベントを記録する。
fn read_bws_secret_by_id(
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

/// `DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH` の state file を読み書きする境界。
///
/// backend stub はこの関数だけを通じて tests 側 fixture state と接続し、fixture 生成や
/// assertion helper の責務を adapter 配下へ持ち込まない。
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

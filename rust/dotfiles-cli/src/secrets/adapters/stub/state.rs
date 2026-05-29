//! `secrets-internal-test-stub` backend が読む shared state schema。
//!
//! adapter 側の stub backend は `DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH` で指定された
//! state file を backend として読み書きする。fixture の生成/検証 helper は tests 側責務。

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use anyhow::Context;

const INTERNAL_STUB_STATE_ENV: &str = "DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH";

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub(super) struct StubState {
    pub(super) key_exists: BTreeMap<u32, bool>,
    pub(super) objects: BTreeMap<(u32, u32), Vec<u8>>,
    pub(super) plaintexts: BTreeMap<(u32, u8), Vec<u8>>,
    pub(super) corrupt: BTreeSet<(u32, u8)>,
    pub(super) include_spare: bool,
    pub(super) requires_pin: bool,
    pub(super) write_events: Vec<String>,
    #[serde(default)]
    pub(super) bws_projects: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) bws_project_secrets: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub(super) bws_secret_values: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    pub(super) bws_fetch_events: Vec<String>,
}

pub(super) fn with_state<T>(
    f: impl FnOnce(&mut StubState) -> crate::Result<T>,
) -> crate::Result<T> {
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

fn endpoint() -> crate::Result<PathBuf> {
    let path = std::env::var(INTERNAL_STUB_STATE_ENV)
        .context("internal stub state path is not configured")?;
    Ok(PathBuf::from(path))
}

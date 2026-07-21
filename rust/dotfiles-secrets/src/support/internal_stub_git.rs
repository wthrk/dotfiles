//! compile-time Git test backend の fixture datastore。

use crate::{
    Result,
    secrets_internal_test_stub_contract::{GIT_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX},
};
use anyhow::Context;
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
#[derive(serde::Deserialize)]
struct Spec {
    #[serde(default)]
    store_exists: bool,
    #[serde(default = "truth")]
    gpg_id_present: bool,
    #[serde(default = "recipients")]
    gpg_id_recipients: Vec<String>,
    #[serde(default = "truth")]
    sample_entry_present: bool,
}
fn truth() -> bool {
    true
}
fn recipients() -> Vec<String> {
    vec!["0123456789ABCDEF0123456789ABCDEF01234567".into()]
}
#[derive(Default)]
struct Store {
    store_exists: bool,
    gpg_id_present: bool,
    gpg_id_recipients: Vec<String>,
    sample_entry_present: bool,
    cloned_remotes: Vec<String>,
}
#[derive(serde::Serialize)]
struct Obs {
    cloned_remotes: Vec<String>,
    store_exists: bool,
}
#[derive(serde::Serialize)]
struct Frame<'a> {
    port: &'static str,
    observation: &'a Obs,
}
static STATE: OnceLock<Mutex<Option<Store>>> = OnceLock::new();
fn with_store<T>(f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    let mut guard = STATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| anyhow::anyhow!("Git internal stub datastore lock is poisoned"))?;
    if guard.is_none() {
        let body = std::env::var(GIT_STUB_SPEC_ENV)
            .context("Git internal stub spec JSON is not configured")?;
        let spec: Spec =
            serde_json::from_str(&body).context("failed to decode Git internal stub spec JSON")?;
        *guard = Some(Store {
            store_exists: spec.store_exists,
            gpg_id_present: spec.gpg_id_present,
            gpg_id_recipients: spec.gpg_id_recipients,
            sample_entry_present: spec.sample_entry_present,
            cloned_remotes: Vec::new(),
        });
    }
    let store = guard.as_mut().expect("initialized");
    let result = f(store)?;
    println!(
        "{STUB_OBSERVATION_PREFIX}{}",
        serde_json::to_string(&Frame {
            port: "git",
            observation: &Obs {
                cloned_remotes: store.cloned_remotes.clone(),
                store_exists: store.store_exists
            }
        })?
    );
    Ok(result)
}
pub(crate) fn store_exists() -> Result<bool> {
    with_store(|store| Ok(store.store_exists))
}
pub(crate) fn inspection() -> Result<(bool, Vec<String>, Option<PathBuf>)> {
    with_store(|store| {
        Ok((
            store.gpg_id_present,
            store.gpg_id_recipients.clone(),
            store
                .sample_entry_present
                .then(|| PathBuf::from("~/.password-store/sample.gpg")),
        ))
    })
}
pub(crate) fn record_clone(remote: &str) -> Result<()> {
    with_store(|store| {
        store.cloned_remotes.push(remote.into());
        store.store_exists = true;
        Ok(())
    })
}

//! compile-time GPG test backend の fixture datastore。

use crate::{
    Result,
    secrets_internal_test_stub_contract::{GPG_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX},
};
use anyhow::Context;
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};
#[derive(serde::Deserialize)]
struct Spec {
    #[serde(default)]
    existing_keys: Vec<String>,
    #[serde(default)]
    keys: BTreeMap<String, Key>,
    #[serde(default)]
    registered_keygrips: Vec<String>,
    #[serde(default = "held")]
    held_recipients: Vec<String>,
    #[serde(default = "truth")]
    store_entry_decryptable: bool,
}
fn truth() -> bool {
    true
}
fn held() -> Vec<String> {
    vec!["0123456789ABCDEF0123456789ABCDEF01234567".into()]
}
#[derive(serde::Deserialize, Clone)]
pub(crate) struct Key {
    #[serde(default = "truth")]
    pub(crate) has_secret_material: bool,
    #[serde(default = "caps")]
    pub(crate) capabilities: Vec<String>,
    pub(crate) keygrip: String,
    pub(crate) ssh_public_key: String,
}
fn caps() -> Vec<String> {
    vec![
        "encryption".into(),
        "authentication".into(),
        "signing".into(),
    ]
}
#[derive(Default)]
struct Store {
    existing_keys: Vec<String>,
    keys: BTreeMap<String, Key>,
    imported: Vec<String>,
    registered: Vec<String>,
    held: Vec<String>,
    decryptable: bool,
}
#[derive(serde::Serialize)]
struct Obs {
    imported_keys: Vec<String>,
    registered_keygrips: Vec<String>,
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
        .map_err(|_| anyhow::anyhow!("GPG internal stub datastore lock is poisoned"))?;
    if guard.is_none() {
        let body = std::env::var(GPG_STUB_SPEC_ENV)
            .context("GPG internal stub spec JSON is not configured")?;
        let spec: Spec =
            serde_json::from_str(&body).context("failed to decode GPG internal stub spec JSON")?;
        *guard = Some(Store {
            existing_keys: spec.existing_keys,
            keys: spec.keys,
            imported: Vec::new(),
            registered: spec.registered_keygrips,
            held: spec.held_recipients,
            decryptable: spec.store_entry_decryptable,
        });
    }
    let store = guard.as_mut().expect("initialized");
    let result = f(store)?;
    println!(
        "{STUB_OBSERVATION_PREFIX}{}",
        serde_json::to_string(&Frame {
            port: "gpg",
            observation: &Obs {
                imported_keys: store.imported.clone(),
                registered_keygrips: store.registered.clone()
            }
        })?
    );
    Ok(result)
}
pub(crate) fn key_exists(fingerprint: &str) -> Result<bool> {
    with_store(|store| Ok(store.existing_keys.iter().any(|key| key == fingerprint)))
}
pub(crate) fn import_key(fingerprint: &str) -> Result<()> {
    with_store(|store| {
        store.imported.push(fingerprint.into());
        Ok(())
    })
}
pub(crate) fn delete_key(fingerprint: &str) -> Result<()> {
    with_store(|store| {
        store.imported.retain(|key| key != fingerprint);
        Ok(())
    })
}
pub(crate) fn key_data(fingerprint: &str) -> Result<Key> {
    with_store(|store| {
        store
            .keys
            .get(fingerprint)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("stub gpg key not found: {fingerprint}"))
    })
}
pub(crate) fn held_recipient(recipient: &str) -> Result<bool> {
    with_store(|store| {
        Ok(store
            .held
            .iter()
            .any(|value| value.eq_ignore_ascii_case(recipient)))
    })
}
pub(crate) fn ensure_store_entry_decryptable() -> Result<()> {
    with_store(|store| {
        if store.decryptable {
            Ok(())
        } else {
            anyhow::bail!("stub password-store entry cannot be decrypted with the restored GPG key")
        }
    })
}
pub(crate) fn test_dek() -> Vec<u8> {
    (0..32).map(|index| index as u8).collect()
}
pub(crate) fn ciphertext_parts(body: Vec<u8>) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    (vec![0; 12], body, vec![0; 16])
}
pub(crate) fn register_keygrip(keygrip: &str) -> Result<()> {
    with_store(|store| {
        if !store.registered.iter().any(|value| value == keygrip) {
            store.registered.push(keygrip.into())
        }
        Ok(())
    })
}
pub(crate) fn registered_ssh_public_keys() -> Result<Vec<String>> {
    with_store(|store| {
        Ok(store
            .keys
            .values()
            .filter(|key| store.registered.iter().any(|value| value == &key.keygrip))
            .map(|key| key.ssh_public_key.clone())
            .collect())
    })
}

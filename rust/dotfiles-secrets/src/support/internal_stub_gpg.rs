//! compile-time GPG test backend の fixture datastore。

use crate::{
    Result,
    domain::{
        gpg_backup::{EnvelopeCiphertext, PrimaryFingerprint},
        gpg_restore::{
            ImportedKeyComposition, Keygrip, OpenSshPublicKey, ResolvedSubkey, SshAgentReadiness,
            SubkeyCapability,
        },
        pass_restore::GpgRecipientId,
    },
    secrets_internal_test_stub_contract::{GPG_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX},
    support::protection::ProtectedSecret,
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

pub(crate) fn export_secret_key(primary: &PrimaryFingerprint) -> Result<ProtectedSecret> {
    ProtectedSecret::from_test_bytes(primary.as_str().as_bytes())
}
pub(crate) fn parse_backup_primary_fingerprint(
    backup: &ProtectedSecret,
) -> Result<PrimaryFingerprint> {
    let value = String::from_utf8(backup.to_test_bytes())
        .context("internal gpg stub backup body is not valid UTF-8")?;
    PrimaryFingerprint::parse(value.trim())
}
pub(crate) fn secret_key_exists(primary: &PrimaryFingerprint) -> Result<bool> {
    key_exists(primary.as_str())
}
pub(crate) fn import_secret_key(backup: &ProtectedSecret) -> Result<PrimaryFingerprint> {
    let value = String::from_utf8(backup.to_test_bytes())
        .context("internal gpg stub backup body is not valid UTF-8")?;
    let primary = PrimaryFingerprint::parse(value.trim())?;
    import_key(primary.as_str())?;
    Ok(primary)
}
pub(crate) fn delete_secret_key(primary: &PrimaryFingerprint) -> Result<()> {
    delete_key(primary.as_str())
}
pub(crate) fn inspect_imported_key(primary: &PrimaryFingerprint) -> Result<ImportedKeyComposition> {
    let key = key_data(primary.as_str())?;
    Ok(ImportedKeyComposition::new(
        key.has_secret_material,
        key.capabilities
            .iter()
            .filter_map(|value| match value.as_str() {
                "encryption" => Some(SubkeyCapability::Encryption),
                "authentication" => Some(SubkeyCapability::Authentication),
                "signing" => Some(SubkeyCapability::Signing),
                _ => None,
            })
            .map(|capability| ResolvedSubkey {
                capability,
                usable: true,
            })
            .collect(),
    ))
}
pub(crate) fn authentication_subkey_keygrip(primary: &PrimaryFingerprint) -> Result<Keygrip> {
    Keygrip::parse(&key_data(primary.as_str())?.keygrip)
}
pub(crate) fn authentication_subkey_ssh_public_key(
    primary: &PrimaryFingerprint,
) -> Result<OpenSshPublicKey> {
    OpenSshPublicKey::parse(&key_data(primary.as_str())?.ssh_public_key)
}
pub(crate) fn secret_key_available_for_recipient(recipient: &GpgRecipientId) -> Result<bool> {
    held_recipient(recipient.as_str())
}
pub(crate) fn can_decrypt_store_entry(_: &std::path::Path) -> Result<()> {
    ensure_store_entry_decryptable()
}
pub(crate) fn generate_dek() -> Result<ProtectedSecret> {
    ProtectedSecret::from_test_bytes(&test_dek())
}
pub(crate) fn encrypt_backup(
    _: &ProtectedSecret,
    backup: &ProtectedSecret,
) -> Result<EnvelopeCiphertext> {
    let (nonce, body, tag) = ciphertext_parts(backup.to_test_bytes());
    EnvelopeCiphertext::new(nonce, body, tag)
}
pub(crate) fn decrypt_backup(
    _: &ProtectedSecret,
    ciphertext: &EnvelopeCiphertext,
) -> Result<ProtectedSecret> {
    ProtectedSecret::from_test_bytes(ciphertext.body())
}
pub(crate) fn register_authentication_subkey(keygrip: &Keygrip) -> Result<()> {
    register_keygrip(keygrip.as_str())
}
pub(crate) fn inspect_ssh_agent(expected: &OpenSshPublicKey) -> Result<SshAgentReadiness> {
    let mut recovery_identity_present = false;
    for key in registered_ssh_public_keys()? {
        let key = OpenSshPublicKey::parse(&key)?;
        if let Some(blob) = key.key_blob() {
            recovery_identity_present |= expected.matches_agent_key_blob(&blob);
        }
    }
    Ok(SshAgentReadiness {
        socket_resolved: true,
        recovery_identity_present,
    })
}

//! `secrets-internal-test-stub` feature 専用の GPG 鍵リング / backup cipher / gpg-agent SSH support の
//! adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で real
//! gpgme / sequoia / sshcontrol backend と差し替わる。integration test はこの module を import せず、同じ
//! `dotfiles` binary を実行する。
//!
//! この stub は GPG port の datastore 境界だけを受け持つ。初期条件は
//! `secrets_internal_test_stub_contract::GPG_STUB_SPEC_ENV` の GPG 専用 spec から private datastore へ
//! 展開し、最終観測 JSON は stdout の sentinel line として書き出す。YubiKey / Bitwarden vault port stub とは
//! state/schema/file を共有しない。
//!
//! cipher stub の backup body 規約: integration test は復号済み backup を「primary fingerprint の
//! lowercase hex 文字列」として与え、keyring stub はその bytes から fingerprint を解決する。これは
//! test-only の観測規約であり、production build には含まれない。

use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use anyhow::Context;

use crate::{
    Result,
    domain::{
        gpg_backup::{EnvelopeCiphertext, PrimaryFingerprint, SecretPrimaryKeyCandidates},
        gpg_restore::{
            ImportedKeyComposition, Keygrip, OpenSshPublicKey, ResolvedSubkey, SubkeyCapability,
        },
        pass_restore::GpgRecipientId,
    },
    ports::gpg::{BackupCipherPort, GpgKeyringPort, SshAgentPort},
    secrets_internal_test_stub_contract::{GPG_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX},
    support::protection::ProtectedSecret,
};

#[derive(serde::Deserialize)]
struct GpgStubSpec {
    /// 既存鍵リングにある primary fingerprint（lowercase hex 40）の一覧。
    #[serde(default)]
    existing_keys: Vec<String>,
    /// import 後に解決できる鍵の構成（fingerprint -> capability/keygrip/ssh）。
    #[serde(default)]
    keys: BTreeMap<String, GpgKeySpec>,
    /// gpg-agent `sshcontrol` に事前登録済みとみなす keygrip（uppercase hex 40）の一覧。restore-gpg の登録
    /// 経路（`register_authentication_subkey`）の初期状態と観測のために保持する。
    #[serde(default)]
    registered_keygrips: Vec<String>,
    /// `.gpg-id` recipient のうち、手元秘密鍵で復号可能（= 秘密鍵を保持）とみなす recipient（uppercase hex）。
    /// 未指定なら既定 recipient だけを保持しているとみなす。
    #[serde(default = "default_held_recipients")]
    held_recipients: Vec<String>,
    /// store サンプル entry を復元済み秘密鍵で復号できるか（`can_decrypt_store_entry` の結果）。
    #[serde(default = "default_true")]
    store_entry_decryptable: bool,
}

/// 既定で手元に保持しているとみなす recipient（Git stub の既定 `.gpg-id` recipient と整合）。
fn default_held_recipients() -> Vec<String> {
    vec!["0123456789ABCDEF0123456789ABCDEF01234567".to_owned()]
}

#[derive(serde::Deserialize, Clone)]
struct GpgKeySpec {
    #[serde(default = "default_true")]
    has_secret_material: bool,
    #[serde(default = "default_capabilities")]
    capabilities: Vec<String>,
    keygrip: String,
    ssh_public_key: String,
}

fn default_true() -> bool {
    true
}

fn default_capabilities() -> Vec<String> {
    vec![
        "encryption".to_owned(),
        "authentication".to_owned(),
        "signing".to_owned(),
    ]
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct GpgDatastore {
    existing_keys: Vec<String>,
    keys: BTreeMap<String, StoredKey>,
    imported: Vec<String>,
    registered_keygrips: Vec<String>,
    held_recipients: Vec<String>,
    store_entry_decryptable: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct StoredKey {
    has_secret_material: bool,
    capabilities: Vec<String>,
    keygrip: String,
    ssh_public_key: String,
}

#[derive(serde::Serialize)]
struct GpgObservation {
    imported_key_count: usize,
    registered_keygrip_count: usize,
}

#[derive(serde::Serialize)]
struct GpgObservationFrame<'a> {
    port: &'static str,
    observation: &'a GpgObservation,
}

static GPG_DATASTORE: OnceLock<Mutex<Option<GpgDatastore>>> = OnceLock::new();

#[derive(Debug)]
struct GpgStubDatastoreLockPoisoned {
    source: DatastoreLockPoisonSource,
}

impl GpgStubDatastoreLockPoisoned {
    fn from_poison<T>(source: std::sync::PoisonError<T>) -> Self {
        Self {
            source: DatastoreLockPoisonSource::from_poison(source),
        }
    }
}

impl std::fmt::Display for GpgStubDatastoreLockPoisoned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GPG internal stub datastore lock is poisoned")
    }
}

impl std::error::Error for GpgStubDatastoreLockPoisoned {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
struct DatastoreLockPoisonSource {
    message: String,
}

impl DatastoreLockPoisonSource {
    fn from_poison<T>(source: std::sync::PoisonError<T>) -> Self {
        Self {
            message: source.to_string(),
        }
    }
}

impl std::fmt::Display for DatastoreLockPoisonSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DatastoreLockPoisonSource {}

/// GPG 鍵リング port の internal backend stub。
#[derive(Default)]
pub(super) struct GpgKeyringStub;

/// backup envelope DEK 暗復号 port の internal backend stub。
#[derive(Default)]
pub(super) struct BackupCipherStub;

/// gpg-agent SSH support port の internal backend stub。
#[derive(Default)]
pub(super) struct SshAgentStub;

impl GpgKeyringPort for GpgKeyringStub {
    fn list_secret_primary_fingerprints(&mut self) -> Result<SecretPrimaryKeyCandidates> {
        with_datastore(|store| {
            let mut keys = store.keys.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            let fingerprints = keys
                .into_iter()
                .map(|key| PrimaryFingerprint::parse(&key))
                .collect::<Result<Vec<_>>>()?;
            Ok(SecretPrimaryKeyCandidates::new(fingerprints))
        })
    }

    fn parse_backup_primary_fingerprint(
        &mut self,
        backup: &ProtectedSecret,
    ) -> Result<PrimaryFingerprint> {
        let hex = String::from_utf8(backup.to_test_bytes())
            .context("internal gpg stub backup body is not valid UTF-8")?;
        PrimaryFingerprint::parse(hex.trim())
    }

    fn secret_key_exists(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<bool> {
        with_datastore(|store| {
            Ok(store
                .existing_keys
                .iter()
                .any(|key| key == primary_fingerprint.as_str()))
        })
    }

    fn import_secret_key(&mut self, backup: &ProtectedSecret) -> Result<PrimaryFingerprint> {
        let hex = String::from_utf8(backup.to_test_bytes())
            .context("internal gpg stub backup body is not valid UTF-8")?;
        let fingerprint = PrimaryFingerprint::parse(hex.trim())?;
        with_datastore(|store| {
            store.imported.push(fingerprint.as_str().to_owned());
            Ok(())
        })?;
        Ok(fingerprint)
    }

    fn delete_secret_key(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<()> {
        with_datastore(|store| {
            store
                .imported
                .retain(|fingerprint| fingerprint != primary_fingerprint.as_str());
            Ok(())
        })
    }

    fn inspect_imported_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ImportedKeyComposition> {
        let key = stored_key(primary_fingerprint)?;
        let subkeys = key
            .capabilities
            .iter()
            .filter_map(|capability| capability_from_str(capability))
            .map(|capability| ResolvedSubkey {
                capability,
                usable: true,
            })
            .collect();
        Ok(ImportedKeyComposition::new(
            key.has_secret_material,
            subkeys,
        ))
    }

    fn authentication_subkey_keygrip(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<Keygrip> {
        let key = stored_key(primary_fingerprint)?;
        Keygrip::parse(&key.keygrip)
    }

    fn authentication_subkey_ssh_public_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<OpenSshPublicKey> {
        let key = stored_key(primary_fingerprint)?;
        OpenSshPublicKey::parse(&key.ssh_public_key)
    }

    fn secret_key_available_for_recipient(&mut self, recipient: &GpgRecipientId) -> Result<bool> {
        with_datastore(|store| {
            Ok(store
                .held_recipients
                .iter()
                .any(|held| held.eq_ignore_ascii_case(recipient.as_str())))
        })
    }

    fn primary_fingerprint_for_recipient(
        &mut self,
        recipient: &GpgRecipientId,
    ) -> Result<Option<PrimaryFingerprint>> {
        with_datastore(|store| {
            let Some(held) = store
                .held_recipients
                .iter()
                .find(|held| held.eq_ignore_ascii_case(recipient.as_str()))
            else {
                return Ok(None);
            };
            if let Some(key) = store
                .keys
                .keys()
                .find(|key| key.eq_ignore_ascii_case(held))
                .cloned()
            {
                return PrimaryFingerprint::parse(&key).map(Some);
            }
            if store.keys.len() == 1 {
                let Some(key) = store.keys.keys().next().cloned() else {
                    anyhow::bail!("single stub key must exist");
                };
                return PrimaryFingerprint::parse(&key).map(Some);
            }
            Ok(None)
        })
    }

    fn can_decrypt_store_entry(&mut self, _entry_path: &std::path::Path) -> Result<()> {
        let decryptable = with_datastore(|store| Ok(store.store_entry_decryptable))?;
        if decryptable {
            Ok(())
        } else {
            anyhow::bail!("stub password-store entry cannot be decrypted with the restored GPG key")
        }
    }
}

impl BackupCipherPort for BackupCipherStub {
    fn decrypt_backup(
        &mut self,
        _dek: &ProtectedSecret,
        ciphertext: &EnvelopeCiphertext,
    ) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(ciphertext.body())
    }
}

impl SshAgentPort for SshAgentStub {
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()> {
        with_datastore(|store| {
            if !store
                .registered_keygrips
                .iter()
                .any(|registered| registered == keygrip.as_str())
            {
                store.registered_keygrips.push(keygrip.as_str().to_owned());
            }
            Ok(())
        })
    }

    fn inspect_ssh_agent(
        &mut self,
        expected_public_key: &OpenSshPublicKey,
    ) -> Result<SshAgentReadiness> {
        // real adapter は agent が列挙する identity の key blob を期待公開鍵の key blob と byte 一致で照合し、
        // 復元鍵 identity が識別可能かを観測する。復元鍵と無関係な既存 identity の有無は観測
        // しない。stub は restore-gpg の register→identify linkage（「期待公開鍵と同一 key blob を持つ鍵の keygrip が
        // SSH key list へ登録済み」なら、その鍵を agent 列挙 identity の 1 つとみなす）を再現し、同じ domain 照合
        // （`matches_agent_key_blob`）で復元鍵の識別可否を判定する。
        let recovery_present = with_datastore(|store| {
            let recovery_present = store
                .keys
                .values()
                .filter(|key| {
                    store
                        .registered_keygrips
                        .iter()
                        .any(|registered| registered == &key.keygrip)
                })
                .filter_map(|key| {
                    OpenSshPublicKey::parse(&key.ssh_public_key)
                        .ok()
                        .and_then(|parsed| parsed.key_blob())
                })
                .any(|blob| expected_public_key.matches_agent_key_blob(&blob));
            Ok(recovery_present)
        })?;
        Ok(SshAgentReadiness {
            socket_resolved: true,
            recovery_identity_present: recovery_present,
        })
    }
}

use crate::domain::gpg_restore::SshAgentReadiness;

fn capability_from_str(value: &str) -> Option<SubkeyCapability> {
    match value {
        "encryption" => Some(SubkeyCapability::Encryption),
        "authentication" => Some(SubkeyCapability::Authentication),
        "signing" => Some(SubkeyCapability::Signing),
        _ => None,
    }
}

fn stored_key(primary_fingerprint: &PrimaryFingerprint) -> Result<StoredKey> {
    with_datastore(|store| {
        store
            .keys
            .get(primary_fingerprint.as_str())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("stub gpg key not found"))
    })
}

fn with_datastore<T>(f: impl FnOnce(&mut GpgDatastore) -> Result<T>) -> Result<T> {
    let datastore = GPG_DATASTORE.get_or_init(|| Mutex::new(None));
    let mut state = datastore
        .lock()
        .map_err(GpgStubDatastoreLockPoisoned::from_poison)?;
    if state.is_none() {
        *state = Some(load_datastore()?);
    }
    let store = state
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("GPG internal stub datastore is not initialized"))?;
    let out = f(store)?;
    write_observation(store)?;
    Ok(out)
}

fn load_datastore() -> Result<GpgDatastore> {
    let body = std::env::var(GPG_STUB_SPEC_ENV)
        .context("GPG internal stub spec JSON is not configured")?;
    let spec: GpgStubSpec =
        serde_json::from_str(&body).context("failed to decode GPG internal stub spec JSON")?;
    Ok(GpgDatastore {
        existing_keys: spec.existing_keys,
        keys: spec
            .keys
            .into_iter()
            .map(|(fingerprint, key)| {
                (
                    fingerprint,
                    StoredKey {
                        has_secret_material: key.has_secret_material,
                        capabilities: key.capabilities,
                        keygrip: key.keygrip,
                        ssh_public_key: key.ssh_public_key,
                    },
                )
            })
            .collect(),
        imported: Vec::new(),
        registered_keygrips: spec.registered_keygrips,
        held_recipients: spec.held_recipients,
        store_entry_decryptable: spec.store_entry_decryptable,
    })
}

fn write_observation(store: &GpgDatastore) -> Result<()> {
    let observation = GpgObservation {
        imported_key_count: store.imported.len(),
        registered_keygrip_count: store.registered_keygrips.len(),
    };
    let frame = GpgObservationFrame {
        port: "gpg",
        observation: &observation,
    };
    println!(
        "{STUB_OBSERVATION_PREFIX}{}",
        serde_json::to_string(&frame)?
    );
    Ok(())
}

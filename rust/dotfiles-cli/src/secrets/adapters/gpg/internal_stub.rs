//! `secrets-internal-test-stub` feature 専用の GPG 鍵リング / backup cipher / gpg-agent SSH support の
//! adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で real
//! gpgme / sequoia / sshcontrol backend と差し替わる。integration test はこの module を import せず、同じ
//! `dotfiles` binary を実行する。
//!
//! この stub は GPG port の datastore 境界だけを受け持つ。初期条件は
//! `secrets_internal_test_stub_contract::GPG_STUB_SPEC_ENV` の GPG 専用 spec から private datastore へ
//! 展開し、最終観測 JSON は stdout の sentinel line として書き出す。YubiKey / BWS port stub とは
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
    secrets::{
        domain::{
            gpg_backup::{EnvelopeCiphertext, PrimaryFingerprint},
            gpg_restore::{
                ImportedKeyComposition, Keygrip, OpenSshPublicKey, ResolvedSubkey, SubkeyCapability,
            },
        },
        ports::gpg::{BackupCipherPort, GpgKeyringPort, SshAgentPort},
        support::protection::ProtectedSecret,
    },
    secrets_internal_test_stub_contract::{GPG_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX},
};

/// stub DEK の固定 byte 長（real backend の AES-256-GCM DEK と同じ 32 bytes）。
const STUB_DEK_LEN: usize = 32;
/// stub の固定 nonce/tag（envelope schema の byte 長に合わせる）。
const STUB_NONCE_LEN: usize = 12;
const STUB_TAG_LEN: usize = 16;

#[derive(serde::Deserialize)]
struct GpgStubSpec {
    /// 既存鍵リングにある primary fingerprint（lowercase hex 40）の一覧。
    #[serde(default)]
    existing_keys: Vec<String>,
    /// import 後に解決できる鍵の構成（fingerprint -> capability/keygrip/ssh）。
    #[serde(default)]
    keys: BTreeMap<String, GpgKeySpec>,
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
    imported_keys: Vec<String>,
    registered_keygrips: Vec<String>,
}

#[derive(serde::Serialize)]
struct GpgObservationFrame<'a> {
    port: &'static str,
    observation: &'a GpgObservation,
}

static GPG_DATASTORE: OnceLock<Mutex<Option<GpgDatastore>>> = OnceLock::new();

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
    fn export_secret_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ProtectedSecret> {
        // export bytes は fingerprint hex 文字列とする（cipher stub の body 規約と整合）。
        ProtectedSecret::from_test_bytes(primary_fingerprint.as_str().as_bytes())
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
}

impl BackupCipherPort for BackupCipherStub {
    fn generate_dek(&mut self) -> Result<ProtectedSecret> {
        let bytes: Vec<u8> = (0..STUB_DEK_LEN).map(|index| index as u8).collect();
        ProtectedSecret::from_test_bytes(&bytes)
    }

    fn encrypt_backup(
        &mut self,
        _dek: &ProtectedSecret,
        backup: &ProtectedSecret,
    ) -> Result<EnvelopeCiphertext> {
        // stub は body をそのまま保持する（DEK round-trip の観測規約）。
        let body = backup.to_test_bytes();
        EnvelopeCiphertext::new(vec![0u8; STUB_NONCE_LEN], body, vec![0u8; STUB_TAG_LEN])
    }

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

    fn inspect_ssh_agent(&mut self, keygrip: &Keygrip) -> Result<SshAgentReadiness> {
        let present = with_datastore(|store| {
            Ok(store
                .registered_keygrips
                .iter()
                .any(|registered| registered == keygrip.as_str()))
        })?;
        Ok(SshAgentReadiness {
            socket_resolved: true,
            authentication_identity_present: present,
        })
    }
}

use crate::secrets::domain::gpg_restore::SshAgentReadiness;

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
            .ok_or_else(|| {
                anyhow::anyhow!("stub gpg key not found: {}", primary_fingerprint.as_str())
            })
    })
}

fn with_datastore<T>(f: impl FnOnce(&mut GpgDatastore) -> Result<T>) -> Result<T> {
    let datastore = GPG_DATASTORE.get_or_init(|| Mutex::new(None));
    let mut state = datastore
        .lock()
        .map_err(|_| anyhow::anyhow!("GPG internal stub datastore lock is poisoned"))?;
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
        registered_keygrips: Vec::new(),
    })
}

fn write_observation(store: &GpgDatastore) -> Result<()> {
    let observation = GpgObservation {
        imported_keys: store.imported.clone(),
        registered_keygrips: store.registered_keygrips.clone(),
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

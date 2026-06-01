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
            pass_restore::GpgRecipientId,
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
    /// restore-pass の recovery 鍵 identity として解決する authentication subkey の OpenSSH 公開鍵。
    /// 未指定なら recovery 鍵を解決できない（restore-gpg 未実行）状態を模す。
    #[serde(default)]
    recovery_ssh_public_key: Option<String>,
    /// gpg-agent `sshcontrol` に事前登録済みとみなす keygrip（uppercase hex 40）の一覧。restore-gpg の登録
    /// 経路（`register_authentication_subkey`）の初期状態と観測のために保持する。
    #[serde(default)]
    registered_keygrips: Vec<String>,
    /// gpg-agent socket が提示する recovery slot の identity の OpenSSH 公開鍵。未指定なら
    /// `recovery_ssh_public_key` を提示する。recovery 鍵と異なる値を指定すると、復元鍵 identity を提示しない
    /// agent（identity 不一致）を模す。
    #[serde(default)]
    agent_identity_ssh_public_key: Option<String>,
    /// gpg-agent が recovery identity に加えて列挙する非 recovery identity の OpenSSH 公開鍵（smartcard /
    /// `Use-for-ssh` 由来鍵など）。1 件でも指定すると、復元鍵以外の identity を列挙する agent を模し、clone 前の
    /// 単一鍵判定で停止させる。
    #[serde(default)]
    agent_extra_identities: Vec<String>,
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
    recovery_ssh_public_key: Option<String>,
    agent_identity_ssh_public_key: Option<String>,
    agent_extra_identities: Vec<String>,
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

    fn resolve_recovery_authentication_ssh_public_key(&mut self) -> Result<OpenSshPublicKey> {
        let line = with_datastore(|store| {
            store
                .recovery_ssh_public_key
                .clone()
                .context("stub recovery GPG identity is not configured (run restore-gpg first)")
        })?;
        OpenSshPublicKey::parse(&line)
    }

    fn secret_key_available_for_recipient(&mut self, recipient: &GpgRecipientId) -> Result<bool> {
        with_datastore(|store| {
            Ok(store
                .held_recipients
                .iter()
                .any(|held| held.eq_ignore_ascii_case(recipient.as_str())))
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

    fn inspect_ssh_agent(
        &mut self,
        expected_public_key: &OpenSshPublicKey,
    ) -> Result<SshAgentReadiness> {
        // real adapter は agent が列挙する identity 全体の key blob を期待公開鍵の key blob と byte 一致で照合し、
        // 復元鍵 identity の有無と復元鍵以外 identity の有無を観測する。stub は agent が列挙する identity 集合を
        // 次の 3 経路から再現し、各 identity を同じ domain 照合（`matches_agent_key_blob`）にかける。
        // - restore-gpg 経路: 「期待公開鍵と同一 key blob を持つ鍵の keygrip が SSH key list へ登録済み」なら、
        //   register→identify の linkage が成立し、その鍵を agent 列挙 identity の 1 つとみなす。
        // - restore-pass の recovery slot: 明示指定があれば `agent_identity_ssh_public_key`、無ければ
        //   `recovery_ssh_public_key` を agent が列挙する 1 identity とみなす。
        // - 非 recovery slot: `agent_extra_identities` を、smartcard / Use-for-ssh 由来の追加 identity として列挙する。
        let (recovery_present, other_present) = with_datastore(|store| {
            let mut recovery_present = false;
            let mut other_present = false;
            let mut observe = |line: &str| {
                if let Some(blob) = OpenSshPublicKey::parse(line)
                    .ok()
                    .and_then(|key| key.key_blob())
                {
                    if expected_public_key.matches_agent_key_blob(&blob) {
                        recovery_present = true;
                    } else {
                        other_present = true;
                    }
                }
            };
            // restore-gpg 経路: 登録済み keygrip を持つ鍵の identity を agent 列挙とみなす。
            for key in store.keys.values() {
                if store
                    .registered_keygrips
                    .iter()
                    .any(|registered| registered == &key.keygrip)
                {
                    observe(&key.ssh_public_key);
                }
            }
            // restore-pass の recovery slot identity。
            if let Some(line) = store
                .agent_identity_ssh_public_key
                .as_deref()
                .or(store.recovery_ssh_public_key.as_deref())
            {
                observe(line);
            }
            // 非 recovery の追加 identity（smartcard / Use-for-ssh 由来鍵）。
            for line in &store.agent_extra_identities {
                observe(line);
            }
            Ok((recovery_present, other_present))
        })?;
        Ok(SshAgentReadiness {
            socket_resolved: true,
            recovery_identity_present: recovery_present,
            other_identity_present: other_present,
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
        registered_keygrips: spec.registered_keygrips,
        recovery_ssh_public_key: spec.recovery_ssh_public_key,
        agent_identity_ssh_public_key: spec.agent_identity_ssh_public_key,
        agent_extra_identities: spec.agent_extra_identities,
        held_recipients: spec.held_recipients,
        store_entry_decryptable: spec.store_entry_decryptable,
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

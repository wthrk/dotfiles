//! `secrets-internal-test-stub` feature 専用の YubiKey adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で
//! real YubiKey backend と差し替わる。integration test はこの module を import せず、Cargo が
//! `secrets-internal-test-stub` feature 付きで事前に build した専用
//! `dotfiles-secrets-internal-test-stub` binary を起動する。その binary は通常 CLI と同じ
//! `dotfiles_cli::dispatch` entrypoint を呼ぶため、command dispatch は同一経路である。
//!
//! 専用 binary target はこの feature を `required-features` にしており、featureless な通常
//! `dotfiles` artifact はその target の代替になれない。従って、この binary に link される
//! YubiKey adapter は本 module に固定され、実機 YubiKey backend へ runtime に fallback する経路はない。
//!
//! この stub は YubiKey port の datastore 境界だけを受け持つ。初期条件は
//! `secrets_internal_test_stub_contract::YUBIKEY_STUB_SPEC_ENV` の YubiKey 専用 spec から private datastore
//! へ展開し、最終観測 JSON は stdout の sentinel line として書き出す。
//! BWS port stub とは state/schema/file を共有しない。

use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use anyhow::Context;

use crate::secrets_internal_test_stub_contract::{STUB_OBSERVATION_PREFIX, YUBIKEY_STUB_SPEC_ENV};
use crate::support::yubikey_backend::{DeviceCandidate, SecretDeviceIo, SelectedSecretDevice};
use crate::{
    Result,
    domain::{
        manifest::SecretManifest,
        piv::{PivApplicationVersion, PivObjectId, SecretStorageSpec},
    },
    support::protection::ProtectedSecret,
};

const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
const BITWARDEN_CLIENT_SECRET_OBJECT_ID: u32 = 0x005f_ff19;

#[derive(serde::Deserialize)]
struct YubiKeyStubSpec {
    yubikeys: Vec<YubiKeyDeviceSpec>,
    /// 複数の CLI process をまたぐ integration test 用の datastore 保存先。
    ///
    /// 値を持つ場合だけ、最初の process は fixture を展開し、以後の process はこの
    /// private datastore を再利用する。production build にはこの backend 自体が含まれない。
    #[serde(default)]
    persistence_path: Option<PathBuf>,
    /// integration test が device open 直後の final datastore を sentinel 観測するための fixture
    /// option。production backend には存在せず、state transition を変えない。
    #[serde(default)]
    observe_on_open: bool,
}

#[derive(serde::Deserialize)]
struct YubiKeyDeviceSpec {
    serial: u32,
    #[serde(flatten)]
    fixture: YubiKeyDeviceFixture,
    #[serde(default)]
    key_metadata_requires_management_auth: bool,
    #[serde(default, rename = "storage_decode_errors")]
    storage_decode_errors: Vec<String>,
    /// PIN-protected management-key state。`b0-default` は production SDK の
    /// `get_protected` error origin が曖昧な場合の fail-closed 回帰 fixture である。
    #[serde(default)]
    management_state: StubManagementState,
    /// feature-only fixture が期待する management PIN。値は observation へ出力しない。
    #[serde(default)]
    expected_management_pin: Option<String>,
}

/// Test-only observable management-key states。production と同じく B0 の
/// `get_protected` `NotFound` は protected-data 不在と確定できないため、変更せず停止する。
#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StubManagementState {
    /// B0: management metadata は default だが `get_protected` の `NotFound`
    /// origin は SDK public API から区別できない。
    B0Default,
    /// PRINTED management key is available after PIN verification.
    #[default]
    Protected,
    WrongPin,
    PinBlocked,
    ProtectedNotFoundNondefault,
    OpaqueError,
    Partial,
}

#[derive(serde::Deserialize)]
#[serde(tag = "fixture", rename_all = "kebab-case")]
enum YubiKeyDeviceFixture {
    Fresh,
    Provisioned,
    ManifestWithMissingSecretObject,
    WritableBitwardenClientSecret,
    ManifestlessBitwardenClientSecret,
    CorruptManifest,
    ManifestWithoutReservedKey,
    ManifestlessReservedCertificate,
    StatusReadFailure,
    Seeded {
        #[serde(rename = "bitwarden-client-secret")]
        bitwarden_client_secret: String,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct YubiKeyDatastore {
    devices: BTreeMap<String, StubDeviceDatastore>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StubDeviceDatastore {
    key_exists: bool,
    #[serde(default)]
    connection_count: u32,
    #[serde(default)]
    physical_verify_count: u32,
    key_metadata_requires_management_auth: bool,
    #[serde(default)]
    slot_public_key_spki: Option<Vec<u8>>,
    reserved_slot_certificate_exists: bool,
    status_read_failure: bool,
    objects: BTreeMap<String, Vec<u8>>,
    secrets: BTreeMap<String, String>,
    corrupt: Vec<String>,
    #[serde(default)]
    management_state: StubManagementState,
    #[serde(default)]
    expected_management_pin: Option<String>,
    #[serde(default)]
    management_pin_matched: bool,
}

/// process 間で test stub の backend state をそのまま保存する wire model。
///
/// これは compile-time test stub 専用の fixture state であり、production datastore ではない。
/// fixture/dummy 値を含む backend state をそのまま保存・復元して、複数 CLI process にまたがる
/// integration test の状態遷移を再現する。
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistentYubiKeyDatastore {
    devices: BTreeMap<String, PersistentStubDeviceDatastore>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistentStubDeviceDatastore {
    key_exists: bool,
    #[serde(default)]
    connection_count: u32,
    #[serde(default)]
    physical_verify_count: u32,
    #[serde(default)]
    key_metadata_requires_management_auth: bool,
    #[serde(default)]
    slot_public_key_spki: Option<Vec<u8>>,
    reserved_slot_certificate_exists: bool,
    status_read_failure: bool,
    objects: BTreeMap<String, Vec<u8>>,
    secrets: BTreeMap<String, String>,
    corrupt: Vec<String>,
    #[serde(default)]
    management_state: StubManagementState,
    #[serde(default)]
    expected_management_pin: Option<String>,
    #[serde(default)]
    management_pin_matched: bool,
}

#[derive(serde::Serialize)]
struct YubiKeyObservation {
    yubikeys: BTreeMap<String, StubDeviceObservation>,
}

#[derive(serde::Serialize)]
struct StubDeviceObservation {
    key_exists: bool,
    connection_count: u32,
    physical_verify_count: u32,
    management_pin_matched: bool,
    stored_secrets: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct YubiKeyObservationFrame<'a> {
    port: &'static str,
    observation: &'a YubiKeyObservation,
}

static YUBIKEY_DATASTORE: OnceLock<Mutex<Option<YubiKeyDatastore>>> = OnceLock::new();

struct TestStubSecretDevice {
    serial: u32,
    management_authenticated: bool,
}

impl SecretDeviceIo for TestStubSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        with_datastore(|store| {
            let device = device_store(store, self.serial)?;
            if device.key_metadata_requires_management_auth && !self.management_authenticated {
                anyhow::bail!("stub YubiKey slot metadata requires management authentication");
            }
            Ok(device.key_exists)
        })
    }

    fn reserved_slot_certificate_exists(&mut self) -> Result<bool> {
        with_datastore(|store| {
            let device = device_store(store, self.serial)?;
            Ok(device.reserved_slot_certificate_exists)
        })
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        PivApplicationVersion {
            major: 5,
            minor: 3,
            patch: 0,
        }
    }

    fn verify_management_pin(&mut self, pin: &ProtectedSecret) -> Result<()> {
        let operation = |store: &mut YubiKeyDatastore| {
            let device = device_store_mut(store, self.serial)?;
            if let Some(expected_pin) = device.expected_management_pin.as_deref() {
                // fixture の期待値とは ProtectedSecret 同士でだけ比較する。PIN bytes を observation、
                // stdout、log に出さず、application→VERIFY の受け渡しだけを test-only に観測する。
                let expected = ProtectedSecret::from_test_bytes(expected_pin.as_bytes())?;
                device.management_pin_matched = pin == &expected;
                if !device.management_pin_matched {
                    anyhow::bail!("stub management PIN does not match the fixture expectation")
                }
            }
            // test-only physical-operation counter。一つの management session は VERIFY を一回送る。
            device.physical_verify_count += 1;
            match device.management_state {
                StubManagementState::WrongPin => {
                    Err(crate::support::protection::yubikey_piv::verify_pin_failure(
                        yubikey::Error::WrongPin { tries: 3 },
                    ))
                }
                StubManagementState::PinBlocked => {
                    Err(crate::support::protection::yubikey_piv::verify_pin_failure(
                        yubikey::Error::WrongPin { tries: 0 },
                    ))
                }
                _ => Ok(()),
            }
        };
        if stub_observes_on_open()? {
            // PIN bytes を露出せず VERIFY counter を observation fixture へ保存する。
            with_datastore_after_write(operation)
        } else {
            with_datastore(operation)
        }
    }

    fn authenticate_protected_management_key(&mut self) -> Result<()> {
        let state = with_datastore(|store| Ok(device_store(store, self.serial)?.management_state))?;
        match state {
            StubManagementState::Protected => {
                self.management_authenticated = true;
                Ok(())
            }
            StubManagementState::B0Default => anyhow::bail!(
                "stub YubiKey B0 management state is ambiguous and cannot bootstrap automatically"
            ),
            StubManagementState::ProtectedNotFoundNondefault => anyhow::bail!(
                "stub YubiKey protected management key is NotFound with non-default metadata"
            ),
            StubManagementState::OpaqueError => {
                anyhow::bail!("stub YubiKey management-key operation failed")
            }
            StubManagementState::Partial => {
                anyhow::bail!("stub YubiKey PIN-only management state is partial")
            }
            StubManagementState::WrongPin | StubManagementState::PinBlocked => {
                anyhow::bail!("stub management-key authentication reached after failed PIN VERIFY")
            }
        }
    }

    fn generate_key(&mut self) -> Result<Vec<u8>> {
        with_datastore_after_write(|store| {
            let device = device_store_mut(store, self.serial)?;
            device.key_exists = true;
            let public_key_spki = SecretManifest::fixture_v2()
                .slot_public_key_spki
                .expect("fixture manifest must include SPKI");
            device.slot_public_key_spki = Some(public_key_spki.clone());
            Ok(public_key_spki)
        })
    }

    fn slot_public_key_spki(&mut self) -> Result<Option<Vec<u8>>> {
        with_datastore(|store| {
            let device = device_store(store, self.serial)?;
            if device.key_metadata_requires_management_auth && !self.management_authenticated {
                anyhow::bail!("stub YubiKey slot metadata requires management authentication");
            }
            Ok(device.slot_public_key_spki.clone())
        })
    }

    fn remember_generated_public_key(&mut self, _public_key: Vec<u8>) {}

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        with_datastore(|store| {
            let device = device_store(store, self.serial)?;
            if device.status_read_failure {
                anyhow::bail!("stub YubiKey device I/O failed while reading storage object");
            }
            let key = object_key(object_id.value());
            if device
                .secrets
                .contains_key(secret_key_for_object(object_id.value()))
            {
                return Ok(Some(encoded_object(object_id.value())));
            }
            Ok(device.objects.get(&key).cloned())
        })
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        with_datastore_after_write(|store| {
            let device = device_store_mut(store, self.serial)?;
            device
                .objects
                .insert(object_key(object_id.value()), value.to_vec());
            Ok(())
        })
    }

    fn empty_object(&mut self, object_id: PivObjectId) -> Result<()> {
        with_datastore_after_write(|store| {
            let device = device_store_mut(store, self.serial)?;
            // Model the fixed SDK's `save_object(id, &mut [])`: it writes an
            // empty `53` value, rather than using a nonexistent object-delete
            // API. The integration test therefore exercises a fresh process
            // observing physical empty objects after clear.
            device
                .objects
                .insert(object_key(object_id.value()), Vec::new());
            let secret_key = secret_key_for_object(object_id.value());
            if !secret_key.is_empty() {
                device.secrets.remove(secret_key);
            }
            Ok(())
        })
    }

    fn clear_reserved_slot_certificate(&mut self) -> Result<()> {
        with_datastore_after_write(|store| {
            device_store_mut(store, self.serial)?.reserved_slot_certificate_exists = false;
            Ok(())
        })
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &ProtectedSecret,
    ) -> Result<Vec<u8>> {
        let value = String::from_utf8(plaintext.to_test_bytes())
            .context("internal stub secret is not valid UTF-8")?;
        with_datastore_after_write(|store| {
            let device = device_store_mut(store, self.serial)?;
            device.key_exists = true;
            device
                .secrets
                .insert(secret_key(storage.secret_id).to_owned(), value);
            Ok(encoded_object(storage_object_id(storage.secret_id)))
        })
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        _encoded: &[u8],
    ) -> Result<ProtectedSecret> {
        let value = with_datastore(|store| {
            let device = device_store(store, self.serial)?;
            let key = secret_key(storage.secret_id);
            if device.corrupt.iter().any(|stored| stored == key) {
                anyhow::bail!("corrupt {key}");
            }
            match device.secrets.get(key) {
                Some(value) if !value.is_empty() => Ok(value.clone()),
                _ => Err(anyhow::anyhow!("missing secret")),
            }
        })?;

        let session = crate::support::protection::SecretSession::start()?;
        let buffer = crate::support::protection::buffer::ProtectedInputBuffer::read_line_from(
            std::io::Cursor::new(value.into_bytes()),
            16 * 1024,
            &session,
        )?;
        buffer.into_protected_secret_line(&session, 16 * 1024, "internal stub secret is too large")
    }

    fn recipient_public_key_fingerprint(&mut self) -> Result<String> {
        // serial ごとに決定的な lowercase hex 64 文字を返し、envelope recipient fixture と照合できるようにする。
        Ok(stub_recipient_fingerprint(self.serial))
    }

    fn wrap_dek(&mut self, dek: &ProtectedSecret) -> Result<Vec<u8>> {
        // stub では DEK を平文 bytes としてそのまま wrapped value に保持し、round-trip を観測可能にする。
        Ok(dek.to_test_bytes())
    }

    fn unwrap_dek(&mut self, wrapped_dek: &[u8]) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(wrapped_dek)
    }
}

/// serial を 64 文字 lowercase hex（recipient `public_key_fingerprint` 相当）へ決定的に写像する。
fn stub_recipient_fingerprint(serial: u32) -> String {
    let prefix = format!("{serial:08x}");
    let mut fingerprint = prefix.repeat(8);
    fingerprint.truncate(64);
    fingerprint
}

pub(crate) fn discover_devices() -> Result<Vec<DeviceCandidate>> {
    with_datastore(|store| {
        store
            .devices
            .keys()
            .map(|serial| {
                let serial = serial
                    .parse::<u32>()
                    .context("internal YubiKey stub datastore contains an invalid serial")?;
                Ok(DeviceCandidate {
                    serial,
                    label: format!("stub-yubikey-{serial}"),
                    piv_version: PivApplicationVersion {
                        major: 5,
                        minor: 3,
                        patch: 0,
                    },
                })
            })
            .collect()
    })
}

pub(crate) fn open_device_by_serial(serial: u32) -> Result<SelectedSecretDevice> {
    if stub_observes_on_open()? {
        with_datastore_after_write(|store| {
            device_store_mut(store, serial)?.connection_count += 1;
            Ok(())
        })?;
    } else {
        with_datastore(|store| {
            device_store_mut(store, serial)?.connection_count += 1;
            Ok(())
        })?;
    }
    Ok(SelectedSecretDevice::new(TestStubSecretDevice {
        serial,
        management_authenticated: false,
    }))
}

fn stub_observes_on_open() -> Result<bool> {
    let body = std::env::var(YUBIKEY_STUB_SPEC_ENV)
        .context("YubiKey internal stub spec JSON is not configured")?;
    Ok(serde_json::from_str::<YubiKeyStubSpec>(&body)
        .context("failed to decode YubiKey internal stub spec JSON")?
        .observe_on_open)
}

fn with_datastore<T>(f: impl FnOnce(&mut YubiKeyDatastore) -> Result<T>) -> Result<T> {
    with_datastore_inner(f, false)
}

/// 更新 port 操作は、integration test に最終 datastore 状態を公開する。
/// `status` を含む読み取り専用操作は、fixture の secret 値を stdout に出力してはならない。
fn with_datastore_after_write<T>(f: impl FnOnce(&mut YubiKeyDatastore) -> Result<T>) -> Result<T> {
    with_datastore_inner(f, true)
}

fn with_datastore_inner<T>(
    f: impl FnOnce(&mut YubiKeyDatastore) -> Result<T>,
    write_observation_after_operation: bool,
) -> Result<T> {
    let datastore = YUBIKEY_DATASTORE.get_or_init(|| Mutex::new(None));
    let mut state = datastore
        .lock()
        .map_err(|_| anyhow::anyhow!("YubiKey internal stub datastore lock is poisoned"))?;
    if state.is_none() {
        *state = Some(load_datastore()?);
    }
    let store = state
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("YubiKey internal stub datastore is not initialized"))?;
    // `observe_on_open` は integration fixture 専用の final-state channel である。mutating operation が
    // error を返しても試行後 datastore を保存・出力する。失敗した PIV VERIFY も physical operation counter
    // には到達し得るため、test は key/object/token mutation 前に停止したことを確認できる。production behavior
    // は変えず、この module 全体は feature-isolated test backend code である。
    let out = f(store);
    if write_observation_after_operation {
        persist_datastore(store)?;
        write_observation(store)?;
    }
    out
}

fn load_datastore() -> Result<YubiKeyDatastore> {
    let body = std::env::var(YUBIKEY_STUB_SPEC_ENV)
        .context("YubiKey internal stub spec JSON is not configured")?;
    let spec: YubiKeyStubSpec =
        serde_json::from_str(&body).context("failed to decode YubiKey internal stub spec JSON")?;
    if let Some(path) = &spec.persistence_path {
        match fs::read(path) {
            Ok(serialized) => {
                let persistent: PersistentYubiKeyDatastore = serde_json::from_slice(&serialized)
                    .context("failed to decode persistent YubiKey internal stub datastore")?;
                return datastore_from_persistent(persistent);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| "failed to read persistent YubiKey internal stub datastore");
            }
        }
    }
    Ok(datastore_from_spec(spec))
}

/// 更新後の test-only datastore state を次の CLI process 用に保存する。
fn persist_datastore(store: &YubiKeyDatastore) -> Result<()> {
    let body = std::env::var(YUBIKEY_STUB_SPEC_ENV)
        .context("YubiKey internal stub spec JSON is not configured")?;
    let spec: YubiKeyStubSpec =
        serde_json::from_str(&body).context("failed to decode YubiKey internal stub spec JSON")?;
    let Some(path) = spec.persistence_path else {
        return Ok(());
    };
    let persistent = persistent_datastore_from(store);
    let serialized = serde_json::to_vec(&persistent)
        .context("failed to encode persistent YubiKey internal stub datastore")?;
    fs::write(path, serialized).context("failed to persist YubiKey internal stub datastore")
}

fn write_observation(store: &YubiKeyDatastore) -> Result<()> {
    let observation = observation_from_datastore(store);
    let frame = YubiKeyObservationFrame {
        port: "yubikey",
        observation: &observation,
    };
    println!(
        "{STUB_OBSERVATION_PREFIX}{}",
        serde_json::to_string(&frame)?
    );
    Ok(())
}

fn datastore_from_spec(spec: YubiKeyStubSpec) -> YubiKeyDatastore {
    let devices = spec
        .yubikeys
        .into_iter()
        .map(|device| {
            (
                device.serial.to_string(),
                device_datastore_from_spec(device),
            )
        })
        .collect();
    YubiKeyDatastore { devices }
}

fn persistent_datastore_from(store: &YubiKeyDatastore) -> PersistentYubiKeyDatastore {
    let devices = store
        .devices
        .iter()
        .map(|(serial, device)| {
            (
                serial.clone(),
                PersistentStubDeviceDatastore {
                    key_exists: device.key_exists,
                    connection_count: device.connection_count,
                    physical_verify_count: device.physical_verify_count,
                    key_metadata_requires_management_auth: device
                        .key_metadata_requires_management_auth,
                    slot_public_key_spki: device.slot_public_key_spki.clone(),
                    reserved_slot_certificate_exists: device.reserved_slot_certificate_exists,
                    status_read_failure: device.status_read_failure,
                    objects: device.objects.clone(),
                    secrets: device.secrets.clone(),
                    corrupt: device.corrupt.clone(),
                    management_state: device.management_state,
                    expected_management_pin: device.expected_management_pin.clone(),
                    management_pin_matched: device.management_pin_matched,
                },
            )
        })
        .collect();
    PersistentYubiKeyDatastore { devices }
}

fn datastore_from_persistent(persistent: PersistentYubiKeyDatastore) -> Result<YubiKeyDatastore> {
    let devices = persistent
        .devices
        .into_iter()
        .map(|(serial, persisted)| {
            Ok((
                serial,
                StubDeviceDatastore {
                    key_exists: persisted.key_exists,
                    connection_count: persisted.connection_count,
                    physical_verify_count: persisted.physical_verify_count,
                    key_metadata_requires_management_auth: persisted
                        .key_metadata_requires_management_auth,
                    slot_public_key_spki: persisted.slot_public_key_spki,
                    reserved_slot_certificate_exists: persisted.reserved_slot_certificate_exists,
                    status_read_failure: persisted.status_read_failure,
                    objects: persisted.objects,
                    secrets: persisted.secrets,
                    corrupt: persisted.corrupt,
                    management_state: persisted.management_state,
                    expected_management_pin: persisted.expected_management_pin,
                    management_pin_matched: persisted.management_pin_matched,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(YubiKeyDatastore { devices })
}

fn device_datastore_from_spec(spec: YubiKeyDeviceSpec) -> StubDeviceDatastore {
    let mut device = match spec.fixture {
        YubiKeyDeviceFixture::Fresh => StubDeviceDatastore::default(),
        YubiKeyDeviceFixture::Provisioned => provisioned_device_datastore(default_secrets()),
        YubiKeyDeviceFixture::ManifestWithMissingSecretObject => {
            let mut device = provisioned_device_datastore(default_secrets());
            device.secrets.remove("bitwarden-client-secret");
            device
        }
        YubiKeyDeviceFixture::WritableBitwardenClientSecret => {
            provisioned_device_datastore(BTreeMap::new())
        }
        YubiKeyDeviceFixture::ManifestlessBitwardenClientSecret => {
            let mut device = StubDeviceDatastore {
                key_exists: true,
                ..StubDeviceDatastore::default()
            };
            device
                .secrets
                .insert("bitwarden-client-secret".to_owned(), "token".to_owned());
            device
        }
        YubiKeyDeviceFixture::CorruptManifest => {
            let mut device = provisioned_device_datastore(default_secrets());
            device.objects.insert(
                object_key(MANIFEST_OBJECT_ID),
                b"not a dotfiles secret manifest".to_vec(),
            );
            device
        }
        YubiKeyDeviceFixture::ManifestWithoutReservedKey => {
            let mut device = provisioned_device_datastore(default_secrets());
            device.key_exists = false;
            // Fixed `yubikey` crate `piv::SlotMetadata::public` is the public
            // key returned by GET METADATA.  A fixture that says the slot key
            // is absent must not retain that observed public key: otherwise it
            // is a contradictory physical observation rather than the known
            // repository state "manifest exists but reserved key is missing".
            // Source: yubikey 0.9.0-pre.0 `piv.rs` `SlotMetadata::public`.
            device.slot_public_key_spki = None;
            device
        }
        YubiKeyDeviceFixture::ManifestlessReservedCertificate => StubDeviceDatastore {
            reserved_slot_certificate_exists: true,
            ..StubDeviceDatastore::default()
        },
        YubiKeyDeviceFixture::StatusReadFailure => StubDeviceDatastore {
            status_read_failure: true,
            ..provisioned_device_datastore(default_secrets())
        },
        YubiKeyDeviceFixture::Seeded {
            bitwarden_client_secret,
        } => provisioned_device_datastore(seeded_secrets(bitwarden_client_secret)),
    };
    device.key_metadata_requires_management_auth = spec.key_metadata_requires_management_auth;
    device.corrupt = spec.storage_decode_errors;
    device.management_state = spec.management_state;
    device.expected_management_pin = spec.expected_management_pin;
    device
}

fn provisioned_device_datastore(secrets: BTreeMap<String, String>) -> StubDeviceDatastore {
    let manifest = SecretManifest::fixture_v2();
    let public_key_spki = manifest
        .slot_public_key_spki
        .clone()
        .expect("fixture manifest must include SPKI");
    let mut objects = BTreeMap::new();
    objects.insert(
        object_key(MANIFEST_OBJECT_ID),
        manifest.encode().expect("fixture manifest"),
    );
    StubDeviceDatastore {
        key_exists: true,
        connection_count: 0,
        physical_verify_count: 0,
        key_metadata_requires_management_auth: false,
        slot_public_key_spki: Some(public_key_spki),
        reserved_slot_certificate_exists: false,
        status_read_failure: false,
        objects,
        secrets,
        corrupt: Vec::new(),
        management_state: StubManagementState::Protected,
        expected_management_pin: None,
        management_pin_matched: false,
    }
}

fn default_secrets() -> BTreeMap<String, String> {
    let mut secrets = BTreeMap::new();
    secrets.insert("bitwarden-client-secret".to_owned(), "token".to_owned());
    secrets
}

fn seeded_secrets(bitwarden_client_secret: String) -> BTreeMap<String, String> {
    let mut seeded = BTreeMap::new();
    seeded.insert(
        "bitwarden-client-secret".to_owned(),
        bitwarden_client_secret,
    );
    seeded
}

fn observation_from_datastore(store: &YubiKeyDatastore) -> YubiKeyObservation {
    let yubikeys = store
        .devices
        .iter()
        .map(|(serial, device)| {
            (
                serial.clone(),
                StubDeviceObservation {
                    key_exists: device.key_exists,
                    connection_count: device.connection_count,
                    physical_verify_count: device.physical_verify_count,
                    management_pin_matched: device.management_pin_matched,
                    stored_secrets: device.secrets.clone(),
                },
            )
        })
        .collect();
    YubiKeyObservation { yubikeys }
}

fn device_store(store: &YubiKeyDatastore, serial: u32) -> Result<&StubDeviceDatastore> {
    store
        .devices
        .get(&serial.to_string())
        .ok_or_else(|| anyhow::anyhow!("stub YubiKey device not found: {serial}"))
}

fn device_store_mut(store: &mut YubiKeyDatastore, serial: u32) -> Result<&mut StubDeviceDatastore> {
    store
        .devices
        .get_mut(&serial.to_string())
        .ok_or_else(|| anyhow::anyhow!("stub YubiKey device not found: {serial}"))
}

fn secret_key_for_object(object_id: u32) -> &'static str {
    match object_id {
        BITWARDEN_CLIENT_SECRET_OBJECT_ID => "bitwarden-client-secret",
        _ => "",
    }
}

fn secret_key(secret_id: u8) -> &'static str {
    match secret_id {
        1 => "bitwarden-client-secret",
        _ => "unknown",
    }
}

fn storage_object_id(secret_id: u8) -> u32 {
    match secret_id {
        1 => BITWARDEN_CLIENT_SECRET_OBJECT_ID,
        _ => MANIFEST_OBJECT_ID,
    }
}

fn object_key(object_id: u32) -> String {
    object_id.to_string()
}

fn encoded_object(object_id: u32) -> Vec<u8> {
    format!("encoded-object-{object_id}").into_bytes()
}

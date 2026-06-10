//! `secrets-internal-test-stub` feature 専用の YubiKey adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で
//! real YubiKey backend と差し替わる。integration test はこの module を import せず、同じ
//! `dotfiles` binary を実行する。
//!
//! この stub は YubiKey port の datastore 境界だけを受け持つ。初期条件は
//! `secrets_internal_test_stub_contract::YUBIKEY_STUB_SPEC_ENV` の YubiKey 専用 spec から private datastore
//! へ展開し、最終観測 JSON は stdout の sentinel line として書き出す。
//! BWS port stub とは state/schema/file を共有しない。

use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use anyhow::Context;

use super::{
    DeviceCandidate, PivApplicationVersion, PivObjectId, ProtectedSecret, Result, SecretDeviceIo,
    SecretStorageSpec, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo, SelectedSecretDevice,
};
use crate::secrets_internal_test_stub_contract::{STUB_OBSERVATION_PREFIX, YUBIKEY_STUB_SPEC_ENV};

const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
const BW_EMAIL_OBJECT_ID: u32 = 0x005f_ff17;
const BW_PASSWORD_OBJECT_ID: u32 = 0x005f_ff18;
const BWS_TOKEN_OBJECT_ID: u32 = 0x005f_ff19;

#[derive(serde::Deserialize)]
struct YubiKeyStubSpec {
    yubikeys: Vec<YubiKeyDeviceSpec>,
    #[serde(default)]
    requires_pin: bool,
}

#[derive(serde::Deserialize)]
struct YubiKeyDeviceSpec {
    serial: u32,
    #[serde(flatten)]
    fixture: YubiKeyDeviceFixture,
    #[serde(default, rename = "storage_decode_errors")]
    storage_decode_errors: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(tag = "fixture", rename_all = "kebab-case")]
enum YubiKeyDeviceFixture {
    Fresh,
    Provisioned,
    WritableBwsAccessToken,
    Seeded {
        #[serde(rename = "bw-email")]
        bw_email: String,
        #[serde(rename = "bw-password")]
        bw_password: String,
        #[serde(rename = "bws-access-token")]
        bws_access_token: String,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct YubiKeyDatastore {
    devices: BTreeMap<String, StubDeviceDatastore>,
    requires_pin: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StubDeviceDatastore {
    key_exists: bool,
    objects: BTreeMap<String, Vec<u8>>,
    secrets: BTreeMap<String, String>,
    corrupt: Vec<String>,
}

#[derive(serde::Serialize)]
struct YubiKeyObservation {
    yubikeys: BTreeMap<String, StubDeviceObservation>,
}

#[derive(serde::Serialize)]
struct StubDeviceObservation {
    key_exists: bool,
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
    pin_verified: bool,
}

impl SelectedDeviceDiscoveryIo for SelectedDeviceAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        discover_devices()
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        open_device_by_serial(serial)
    }
}

impl SecretDeviceIo for TestStubSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        with_datastore(|store| {
            let device = device_store(store, self.serial)?;
            Ok(device.key_exists)
        })
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        PivApplicationVersion {
            major: 5,
            minor: 3,
            patch: 0,
        }
    }

    fn pin_retries(&mut self) -> Result<u8> {
        Ok(1)
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        with_datastore(|store| {
            let device = device_store_mut(store, self.serial)?;
            device.key_exists = true;
            Ok(())
        })
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        with_datastore(|store| {
            let device = device_store(store, self.serial)?;
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
        with_datastore(|store| {
            let device = device_store_mut(store, self.serial)?;
            device
                .objects
                .insert(object_key(object_id.value()), value.to_vec());
            Ok(())
        })
    }

    fn requires_pin_input(&self) -> bool {
        with_datastore(|store| Ok(store.requires_pin)).unwrap_or(false)
    }

    fn verify_pin(&mut self, _pin: &ProtectedSecret) -> Result<()> {
        self.pin_verified = true;
        Ok(())
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &ProtectedSecret,
    ) -> Result<Vec<u8>> {
        let value = String::from_utf8(plaintext.to_test_bytes())
            .context("internal stub secret is not valid UTF-8")?;
        with_datastore(|store| {
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
            device
                .secrets
                .get(key)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing secret"))
        })?;

        let session = crate::secrets::support::protection::SecretSession::start()?;
        let buffer =
            crate::secrets::support::protection::buffer::ProtectedInputBuffer::read_line_from(
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

fn discover_devices() -> Result<Vec<DeviceCandidate>> {
    with_datastore(|store| {
        Ok(store
            .devices
            .keys()
            .filter_map(|serial| serial.parse::<u32>().ok())
            .map(|serial| DeviceCandidate {
                serial,
                label: format!("stub-yubikey-{serial}"),
            })
            .collect())
    })
}

fn open_device_by_serial(serial: u32) -> Result<SelectedSecretDevice> {
    Ok(SelectedSecretDevice::new(TestStubSecretDevice {
        serial,
        pin_verified: false,
    }))
}

fn with_datastore<T>(f: impl FnOnce(&mut YubiKeyDatastore) -> Result<T>) -> Result<T> {
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
    let out = f(store)?;
    write_observation(store)?;
    Ok(out)
}

fn load_datastore() -> Result<YubiKeyDatastore> {
    let body = std::env::var(YUBIKEY_STUB_SPEC_ENV)
        .context("YubiKey internal stub spec JSON is not configured")?;
    let spec: YubiKeyStubSpec =
        serde_json::from_str(&body).context("failed to decode YubiKey internal stub spec JSON")?;
    Ok(datastore_from_spec(spec))
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
    YubiKeyDatastore {
        devices,
        requires_pin: spec.requires_pin,
    }
}

fn device_datastore_from_spec(spec: YubiKeyDeviceSpec) -> StubDeviceDatastore {
    let mut device = match spec.fixture {
        YubiKeyDeviceFixture::Fresh => StubDeviceDatastore::default(),
        YubiKeyDeviceFixture::Provisioned => provisioned_device_datastore(default_secrets()),
        YubiKeyDeviceFixture::WritableBwsAccessToken => {
            let mut secrets = BTreeMap::new();
            secrets.insert("bw-email".to_owned(), "u@example.com".to_owned());
            secrets.insert("bw-password".to_owned(), "pw".to_owned());
            provisioned_device_datastore(secrets)
        }
        YubiKeyDeviceFixture::Seeded {
            bw_email,
            bw_password,
            bws_access_token,
        } => provisioned_device_datastore(seeded_secrets(bw_email, bw_password, bws_access_token)),
    };
    device.corrupt = spec.storage_decode_errors;
    device
}

fn provisioned_device_datastore(secrets: BTreeMap<String, String>) -> StubDeviceDatastore {
    let mut objects = BTreeMap::new();
    objects.insert(
        object_key(MANIFEST_OBJECT_ID),
        br#"{"version":1,"app":"dotfiles.secret-recovery"}"#.to_vec(),
    );
    StubDeviceDatastore {
        key_exists: true,
        objects,
        secrets,
        corrupt: Vec::new(),
    }
}

fn default_secrets() -> BTreeMap<String, String> {
    let mut secrets = BTreeMap::new();
    secrets.insert("bw-email".to_owned(), "u@example.com".to_owned());
    secrets.insert("bw-password".to_owned(), "pw".to_owned());
    secrets.insert("bws-access-token".to_owned(), "token".to_owned());
    secrets
}

fn seeded_secrets(
    bw_email: String,
    bw_password: String,
    bws_access_token: String,
) -> BTreeMap<String, String> {
    let mut seeded = BTreeMap::new();
    seeded.insert("bw-email".to_owned(), bw_email);
    seeded.insert("bw-password".to_owned(), bw_password);
    seeded.insert("bws-access-token".to_owned(), bws_access_token);
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
        BW_EMAIL_OBJECT_ID => "bw-email",
        BW_PASSWORD_OBJECT_ID => "bw-password",
        BWS_TOKEN_OBJECT_ID => "bws-access-token",
        _ => "",
    }
}

fn secret_key(secret_id: u8) -> &'static str {
    match secret_id {
        1 => "bw-email",
        2 => "bw-password",
        3 => "bws-access-token",
        _ => "unknown",
    }
}

fn storage_object_id(secret_id: u8) -> u32 {
    match secret_id {
        1 => BW_EMAIL_OBJECT_ID,
        2 => BW_PASSWORD_OBJECT_ID,
        3 => BWS_TOKEN_OBJECT_ID,
        _ => MANIFEST_OBJECT_ID,
    }
}

fn object_key(object_id: u32) -> String {
    object_id.to_string()
}

fn encoded_object(object_id: u32) -> Vec<u8> {
    format!("encoded-object-{object_id}").into_bytes()
}

//! `secrets-internal-test-stub` feature 専用の YubiKey adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で
//! real YubiKey backend と差し替わる。integration test はこの module を import せず、同じ
//! `dotfiles` binary を実行する。
//!
//! この stub は YubiKey port の datastore 境界だけを受け持つ。初期 datastore は
//! `DOTFILES_SECRETS_YUBIKEY_STUB_DATASTORE_JSON` から読み、最終 datastore は
//! `DOTFILES_SECRETS_YUBIKEY_STUB_OUTPUT_PATH` へ JSON として書き出す。BWS port stub とは
//! state/schema/file を共有しない。

use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::Context;

use super::{
    DeviceCandidate, PivApplicationVersion, PivObjectId, ProtectedSecret, Result, SecretDeviceIo,
    SecretStorageSpec, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo, SelectedSecretDevice,
};

const YUBIKEY_STUB_DATASTORE_ENV: &str = "DOTFILES_SECRETS_YUBIKEY_STUB_DATASTORE_JSON";
const YUBIKEY_STUB_OUTPUT_ENV: &str = "DOTFILES_SECRETS_YUBIKEY_STUB_OUTPUT_PATH";
const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
const BW_EMAIL_OBJECT_ID: u32 = 0x005f_ff17;
const BW_PASSWORD_OBJECT_ID: u32 = 0x005f_ff18;
const BWS_ACCESS_TOKEN_OBJECT_ID: u32 = 0x005f_ff19;

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
    let mut store = load_datastore()?;
    let out = f(&mut store)?;
    write_observed_datastore(&store)?;
    Ok(out)
}

fn load_datastore() -> Result<YubiKeyDatastore> {
    let path = output_path()?;
    if path.exists() {
        let body = fs::read(&path)?;
        return serde_json::from_slice(&body)
            .context("failed to decode observed YubiKey internal stub datastore JSON");
    }
    let body = std::env::var(YUBIKEY_STUB_DATASTORE_ENV)
        .context("YubiKey internal stub datastore JSON is not configured")?;
    serde_json::from_str(&body).context("failed to decode YubiKey internal stub datastore JSON")
}

fn write_observed_datastore(store: &YubiKeyDatastore) -> Result<()> {
    let path = output_path()?;
    let body = serde_json::to_vec_pretty(store)?;
    fs::write(path, body)?;
    Ok(())
}

fn output_path() -> Result<PathBuf> {
    let path = std::env::var(YUBIKEY_STUB_OUTPUT_ENV)
        .context("YubiKey internal stub output path is not configured")?;
    Ok(PathBuf::from(path))
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
        BWS_ACCESS_TOKEN_OBJECT_ID => "bws-access-token",
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
        3 => BWS_ACCESS_TOKEN_OBJECT_ID,
        _ => MANIFEST_OBJECT_ID,
    }
}

fn object_key(object_id: u32) -> String {
    object_id.to_string()
}

fn encoded_object(object_id: u32) -> Vec<u8> {
    format!("encoded-object-{object_id}").into_bytes()
}

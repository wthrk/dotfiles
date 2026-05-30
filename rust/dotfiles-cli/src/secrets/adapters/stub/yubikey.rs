//! `secrets-internal-test-stub` feature 専用の file-backed YubiKey adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で
//! real YubiKey backend と差し替わる。integration test はこの module を import せず、同じ
//! `dotfiles` binary を実行し、fixture が作る `DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH` の
//! state file を backend として共有する。

use std::fs;

use anyhow::Context;

use super::{
    DeviceCandidate, PivApplicationVersion, PivObjectId, ProtectedSecret, Result, SecretDeviceIo,
    SecretStorageSpec, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo, SelectedSecretDevice,
};

const INTERNAL_STUB_STATE_ENV: &str = "DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH";
const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
const BW_EMAIL_OBJECT_ID: u32 = 0x005f_ff17;
const BW_PASSWORD_OBJECT_ID: u32 = 0x005f_ff18;
const BWS_ACCESS_TOKEN_OBJECT_ID: u32 = 0x005f_ff19;
const PRIMARY_SERIAL: u32 = 2001;
const SPARE_SERIAL: u32 = 2002;

#[derive(serde::Serialize, serde::Deserialize, Default)]
/// adapter stub が state file から読む最小 schema。
///
/// tests 側の `cli_stub_state` は fixture 生成と assertion を担い、この型は YubiKey backend
/// port 実装に必要な device/object/plaintext state だけを保持する。
struct StubState {
    key_exists: std::collections::BTreeMap<u32, bool>,
    objects: std::collections::BTreeMap<(u32, u32), Vec<u8>>,
    plaintexts: std::collections::BTreeMap<(u32, u8), Vec<u8>>,
    corrupt: std::collections::BTreeSet<(u32, u8)>,
    include_spare: bool,
    requires_pin: bool,
    write_events: Vec<String>,
    #[serde(default)]
    bws_projects: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    bws_project_secrets:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    bws_secret_values: std::collections::BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    bws_fetch_events: Vec<String>,
}

struct TestStubSecretDevice {
    serial: u32,
    pin_verified: bool,
}

/// `DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH` の state file を読み書きする境界。
///
/// backend stub はこの関数だけを通じて tests 側 fixture state と接続し、fixture 生成や
/// assertion helper の責務を adapter 配下へ持ち込まない。
fn with_state<T>(f: impl FnOnce(&mut StubState) -> Result<T>) -> Result<T> {
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

/// file-backed internal stub から device 候補を取得し、adapter 境界型へ翻訳する。
fn discover_devices() -> Result<Vec<DeviceCandidate>> {
    with_state(|state| {
        let mut out = vec![DeviceCandidate {
            serial: PRIMARY_SERIAL,
            label: format!("stub-yubikey-{PRIMARY_SERIAL}"),
        }];
        if state.include_spare {
            out.push(DeviceCandidate {
                serial: SPARE_SERIAL,
                label: format!("stub-yubikey-{SPARE_SERIAL}"),
            });
        }
        Ok(out)
    })
}

/// 指定 serial の stub device を開き、`SelectedSecretDevice` 境界へ包んで返す。
fn open_device_by_serial(serial: u32) -> Result<SelectedSecretDevice> {
    Ok(SelectedSecretDevice::new(TestStubSecretDevice {
        serial,
        pin_verified: false,
    }))
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
        with_state(|state| Ok(state.key_exists.get(&self.serial).copied().unwrap_or(false)))
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
        with_state(|state| {
            state.key_exists.insert(self.serial, true);
            Ok(())
        })
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        with_state(|state| {
            Ok(state
                .objects
                .get(&(self.serial, object_id.value()))
                .cloned())
        })
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        with_state(|state| {
            state
                .objects
                .insert((self.serial, object_id.value()), value.to_vec());
            Ok(())
        })
    }

    fn requires_pin_input(&self) -> bool {
        with_state(|state| Ok(state.requires_pin)).unwrap_or(false)
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
        let bytes = plaintext.to_test_bytes();
        with_state(|state| {
            state.key_exists.insert(self.serial, true);
            state
                .plaintexts
                .insert((self.serial, storage.secret_id), bytes);
            if let Some(secret_name) = secret_name(storage.secret_id) {
                state.write_events.push(format!(
                    "DOTFILES_TEST_STUB_WRITE serial={} name={} value=<redacted>",
                    self.serial, secret_name
                ));
            }
            Ok(encoded_object(storage.secret_id))
        })
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        _encoded: &[u8],
    ) -> Result<ProtectedSecret> {
        let plaintext = with_state(|state| {
            if state.corrupt.contains(&(self.serial, storage.secret_id)) {
                let name = secret_name(storage.secret_id).unwrap_or("unknown");
                anyhow::bail!("corrupt {name}");
            }
            state
                .plaintexts
                .get(&(self.serial, storage.secret_id))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing secret"))
        })?;

        let session = crate::secrets::support::protection::SecretSession::start()?;
        let buffer =
            crate::secrets::support::protection::buffer::ProtectedInputBuffer::read_line_from(
                std::io::Cursor::new(plaintext),
                16 * 1024,
                &session,
            )?;
        buffer.into_protected_secret_line(&session, 16 * 1024, "internal stub secret is too large")
    }
}

fn endpoint() -> Result<std::path::PathBuf> {
    let path = std::env::var(INTERNAL_STUB_STATE_ENV)
        .context("internal stub state path is not configured")?;
    Ok(std::path::PathBuf::from(path))
}

fn secret_name(secret_id: u8) -> Option<&'static str> {
    match secret_id {
        1 => Some("bw-email"),
        2 => Some("bw-password"),
        3 => Some("bws-access-token"),
        _ => None,
    }
}

fn encoded_object(secret_id: u8) -> Vec<u8> {
    let object_id = match secret_id {
        1 => BW_EMAIL_OBJECT_ID,
        2 => BW_PASSWORD_OBJECT_ID,
        3 => BWS_ACCESS_TOKEN_OBJECT_ID,
        _ => MANIFEST_OBJECT_ID,
    };
    format!("encoded-object-{object_id}").into_bytes()
}

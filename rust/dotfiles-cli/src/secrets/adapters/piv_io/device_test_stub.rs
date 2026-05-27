use std::{cell::RefCell, collections::BTreeMap, env, rc::Rc};

use crate::{
    Result,
    secrets::{
        domain::{
            blob::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob},
            manifest::SecretManifest,
            material::SecretMaterial,
            piv::{PIV_PIN_MAX_LEN, PIV_PIN_MIN_LEN, PivObjectId, SecretName},
        },
        ports::{DeviceCandidate, DeviceSelectionPort, SecretDevice},
    },
};
use aes_gcm::{AeadInPlace, Aes256Gcm, KeyInit};
use dotfiles_cli_secrets_test_contract::{
    CORRUPT_SECRET_ENV, PRIMARY_SERIAL, PRIMARY_STUB_STATE_ENV, READ_PIN_FROM_TTY_ENV,
    SEED_BW_EMAIL_ENV, SEED_BW_PASSWORD_ENV, SEED_BWS_ACCESS_TOKEN_ENV, SPARE_SERIAL,
    SPARE_STUB_STATE_ENV, STUB_STATE_ENV, StubState, format_write_event,
};

const REDACTED_WRITE_VALUE: &str = "<redacted>";
const STUB_WRAP_PREFIX: &[u8] = b"dotfiles-stub-wrapped-v1:";

fn wrap_stub_content_key(key: &SecretMaterial) -> Vec<u8> {
    key.with_secret(|bytes| {
        let mut wrapped = Vec::with_capacity(STUB_WRAP_PREFIX.len() + bytes.len());
        wrapped.extend_from_slice(STUB_WRAP_PREFIX);
        wrapped.extend(bytes.iter().map(|byte| byte ^ 0xa5));
        wrapped
    })
}

fn unwrap_stub_content_key(wrapped_key: &[u8]) -> Result<SecretMaterial> {
    let Some(masked) = wrapped_key.strip_prefix(STUB_WRAP_PREFIX) else {
        anyhow::bail!("invalid stub-wrapped content key");
    };
    let mut key = SecretMaterial::new(masked.len())?;
    key.with_secret_mut(|bytes| {
        for (dst, source) in bytes.iter_mut().zip(masked.iter()) {
            *dst = *source ^ 0xa5;
        }
    });
    Ok(key)
}

/// env 契約の state 文字列を `StubState` へ変換する。
///
/// 無効値を panic で止めず `None` へ落とすことで、fixture 未指定時に default state へ戻せるようにする。
fn parse_state_env(key: &str) -> Option<StubState> {
    env::var(key)
        .ok()
        .and_then(|value| StubState::parse_env_value(value.as_str()))
}

/// serial ごとの既定 state を返す。
///
/// primary は既存運用データ読み出し検証が多いため `Provisioned`、spare は登録フロー検証のため `Fresh` を既定にする。
fn default_state_for_serial(serial: u32) -> StubState {
    match serial {
        PRIMARY_SERIAL => StubState::Provisioned,
        SPARE_SERIAL => StubState::Fresh,
        _ => StubState::Fresh,
    }
}

fn make_seed_blob(name: SecretName, plaintext: &[u8]) -> Vec<u8> {
    let nonce = [0u8; NONCE_LEN];
    let Ok(content_key) = SecretMaterial::copy_from_slice(&[0u8; CONTENT_KEY_LEN]) else {
        return Vec::new();
    };
    let Ok(mut ciphertext) = SecretMaterial::copy_from_slice(plaintext) else {
        return Vec::new();
    };
    let Ok(cipher) = content_key.with_bytes(Aes256Gcm::new_from_slice) else {
        return Vec::new();
    };
    let Ok(tag) = ciphertext.with_secret_mut(|bytes| {
        cipher.encrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(&nonce),
            &name.additional_data(PRIMARY_SERIAL),
            bytes,
        )
    }) else {
        return Vec::new();
    };
    let Ok(tag) = tag.as_slice().try_into() else {
        return Vec::new();
    };
    let blob = SecretBlob {
        name,
        nonce,
        wrapped_key: wrap_stub_content_key(&content_key),
        ciphertext,
        tag,
    };
    blob.encode().unwrap_or_default()
}

/// `SecretDevice` 契約を in-memory 状態で実装する test 専用 adapter。
pub(crate) struct TestStubDeviceAdapter {
    devices: BTreeMap<u32, Rc<RefCell<TestStubDeviceState>>>,
    read_pin_from_tty: bool,
}

impl TestStubDeviceAdapter {
    /// テスト契約 env を読み取り、serial ごとの in-memory PIV 状態を初期化する。
    ///
    /// 同一 production command path を維持したまま dependency selection だけを切り替えるため、
    /// state 制御はすべて env 契約から受け取る。
    fn production() -> Self {
        let default_state_primary = default_state_for_serial(PRIMARY_SERIAL);
        let default_state_spare = default_state_for_serial(SPARE_SERIAL);
        let default_state = parse_state_env(STUB_STATE_ENV).unwrap_or(default_state_primary);
        let primary_state = parse_state_env(PRIMARY_STUB_STATE_ENV).unwrap_or(default_state);
        let spare_state = parse_state_env(SPARE_STUB_STATE_ENV).unwrap_or(default_state_spare);

        Self {
            devices: BTreeMap::from([
                (
                    PRIMARY_SERIAL,
                    Rc::new(RefCell::new(
                        TestStubDeviceState::from_state(primary_state).with_seed_env(),
                    )),
                ),
                (
                    SPARE_SERIAL,
                    Rc::new(RefCell::new(TestStubDeviceState::from_state(spare_state))),
                ),
            ]),
            read_pin_from_tty: env::var(READ_PIN_FROM_TTY_ENV).as_deref() == Ok("true"),
        }
    }
}

impl Default for TestStubDeviceAdapter {
    fn default() -> Self {
        Self::production()
    }
}

impl DeviceSelectionPort for TestStubDeviceAdapter {
    type Device = TestStubSecretDevice;
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        Ok(self
            .devices
            .keys()
            .map(|serial| DeviceCandidate {
                serial: *serial,
                label: format!("stub-yubikey-{serial}"),
            })
            .collect())
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        let state = self
            .devices
            .entry(serial)
            .or_insert_with(|| Rc::new(RefCell::new(TestStubDeviceState::fresh())))
            .clone();
        Ok(TestStubSecretDevice {
            serial,
            state,
            read_pin_from_tty: self.read_pin_from_tty,
            pin_verified: false,
        })
    }
}

/// stub state を保持しつつ `SecretDevice` 振る舞いを提供する仮想デバイス。
pub(crate) struct TestStubSecretDevice {
    serial: u32,
    state: Rc<RefCell<TestStubDeviceState>>,
    read_pin_from_tty: bool,
    pin_verified: bool,
}

/// serial ごとの object map と初期化状態を保持する stub 内部状態。
#[derive(Clone)]
struct TestStubDeviceState {
    key_exists: bool,
    objects: BTreeMap<PivObjectId, Vec<u8>>,
}

impl TestStubDeviceState {
    fn from_state(state: StubState) -> Self {
        match state {
            StubState::Fresh => Self::fresh(),
            StubState::Initialized => Self::initialized(),
            StubState::Provisioned => Self::provisioned(),
            StubState::WritableBwsAccessToken => Self::writable_bws_access_token(),
        }
    }

    fn fresh() -> Self {
        Self {
            key_exists: false,
            objects: BTreeMap::new(),
        }
    }

    fn initialized() -> Self {
        let mut state = Self::fresh();
        state.key_exists = true;
        if let Ok(bytes) = SecretManifest::expected().encode() {
            state.objects.insert(PivObjectId::MANIFEST, bytes);
        }
        state
    }

    fn provisioned() -> Self {
        let mut state = Self::initialized();
        state.objects.insert(
            SecretName::BwEmail.object_id(),
            make_seed_blob(SecretName::BwEmail, b"u@example.com"),
        );
        state.objects.insert(
            SecretName::BwPassword.object_id(),
            make_seed_blob(SecretName::BwPassword, b"pw"),
        );
        state.objects.insert(
            SecretName::BwsAccessToken.object_id(),
            make_seed_blob(SecretName::BwsAccessToken, b"token"),
        );
        state
    }

    fn writable_bws_access_token() -> Self {
        let mut state = Self::initialized();
        state
            .objects
            .remove(&SecretName::BwsAccessToken.object_id());
        state
    }

    fn with_seed_env(mut self) -> Self {
        let seeds = [
            (SecretName::BwEmail, SEED_BW_EMAIL_ENV),
            (SecretName::BwPassword, SEED_BW_PASSWORD_ENV),
            (SecretName::BwsAccessToken, SEED_BWS_ACCESS_TOKEN_ENV),
        ];
        for (name, env_key) in seeds {
            if let Ok(value) = env::var(env_key) {
                self.key_exists = true;
                if !self.objects.contains_key(&PivObjectId::MANIFEST) {
                    if let Ok(bytes) = SecretManifest::expected().encode() {
                        self.objects.insert(PivObjectId::MANIFEST, bytes);
                    }
                }
                self.objects
                    .insert(name.object_id(), make_seed_blob(name, value.as_bytes()));
            }
        }
        self
    }
}

impl SecretDevice for TestStubSecretDevice {
    fn serial(&self) -> u32 {
        self.serial
    }

    fn key_exists(&mut self) -> Result<bool> {
        Ok(self.state.borrow().key_exists)
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        self.state.borrow_mut().key_exists = true;
        Ok(())
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        if let Some(name) = SecretName::iter().find(|name| name.object_id() == object_id) {
            if env::var(CORRUPT_SECRET_ENV).as_deref() == Ok(name.to_string().as_str()) {
                return Ok(Some(b"not-json".to_vec()));
            }
        }
        Ok(self.state.borrow().objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.state
            .borrow_mut()
            .objects
            .insert(object_id, value.to_vec());
        if let Some(name) = SecretName::iter().find(|name| name.object_id() == object_id) {
            eprintln!(
                "{}",
                format_write_event(self.serial, &name.to_string(), REDACTED_WRITE_VALUE)
            );
        }
        Ok(())
    }

    fn wrap_key(&mut self, key: &SecretMaterial) -> Result<Vec<u8>> {
        Ok(wrap_stub_content_key(key))
    }

    fn requires_pin_input(&self) -> bool {
        self.read_pin_from_tty
    }

    /// PIN 検証の境界条件を実機と同じ長さ制約に揃える。
    ///
    /// same-route 検証で stub だけが緩い入力を許可すると分岐網羅が崩れるため、
    /// 最低限の PIN byte 長チェックは adapter 契約として常に適用する。
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        if !(PIV_PIN_MIN_LEN..=PIV_PIN_MAX_LEN).contains(&pin.len()) {
            anyhow::bail!("YubiKey PIN must be 6 to 8 bytes");
        }
        self.pin_verified = true;
        Ok(())
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<SecretMaterial> {
        if self.read_pin_from_tty && !self.pin_verified {
            anyhow::bail!("YubiKey PIN must be verified before reading stored secrets");
        }
        unwrap_stub_content_key(wrapped_key)
    }
}

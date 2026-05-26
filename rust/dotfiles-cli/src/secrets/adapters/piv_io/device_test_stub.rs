use std::{cell::RefCell, collections::BTreeMap, env, rc::Rc};

use anyhow::Context;
use zeroize::Zeroizing;

use crate::{
    Result,
    secrets::{
        domain::{
            CONTENT_KEY_LEN, NONCE_LEN, PivObjectId, SecretBlob, SecretManifest, SecretName,
            TAG_LEN, encode_manifest, ensure_secret_value_non_empty,
        },
        ports::{DeviceSelectionPort, RandomBytesPort, SecretDevice},
    },
};

use super::{DiscoveredDevice, SecretDeviceExt};

const STUB_STATE_ENV: &str = "DOTFILES_TEST_STUB_STATE";
const PRIMARY_STUB_STATE_ENV: &str = "DOTFILES_TEST_STUB_STATE_2001";
const SPARE_STUB_STATE_ENV: &str = "DOTFILES_TEST_STUB_STATE_2002";
const SEED_BW_EMAIL_ENV: &str = "DOTFILES_TEST_STUB_SEED_BW_EMAIL";
const SEED_BW_PASSWORD_ENV: &str = "DOTFILES_TEST_STUB_SEED_BW_PASSWORD";
const SEED_BWS_ACCESS_TOKEN_ENV: &str = "DOTFILES_TEST_STUB_SEED_BWS_ACCESS_TOKEN";
const CORRUPT_SECRET_ENV: &str = "DOTFILES_TEST_STUB_CORRUPT_SECRET";
const READ_PIN_FROM_TTY_ENV: &str = "DOTFILES_TEST_STUB_READ_PIN_FROM_TTY";
const WRITE_EVENT_PREFIX: &str = "DOTFILES_TEST_STUB_WRITE";

#[derive(Clone, Copy)]
enum StubState {
    Fresh,
    Initialized,
    Provisioned,
    WritableBwsAccessToken,
}

impl StubState {
    fn from_env_value(value: &str) -> Option<Self> {
        match value {
            "fresh" => Some(Self::Fresh),
            "initialized" => Some(Self::Initialized),
            "provisioned" => Some(Self::Provisioned),
            "writable-bws-access-token" => Some(Self::WritableBwsAccessToken),
            _ => None,
        }
    }
}

fn parse_state_env(key: &str) -> Option<StubState> {
    env::var(key)
        .ok()
        .and_then(|value| StubState::from_env_value(value.as_str()))
}

fn seed_value_for(name: SecretName) -> Vec<u8> {
    match name {
        SecretName::BwEmail => env::var(SEED_BW_EMAIL_ENV)
            .unwrap_or_else(|_| "u@example.com".to_owned())
            .into_bytes(),
        SecretName::BwPassword => env::var(SEED_BW_PASSWORD_ENV)
            .unwrap_or_else(|_| "pw".to_owned())
            .into_bytes(),
        SecretName::BwsAccessToken => env::var(SEED_BWS_ACCESS_TOKEN_ENV)
            .unwrap_or_else(|_| "token".to_owned())
            .into_bytes(),
    }
}

fn corrupt_secret_name() -> Option<SecretName> {
    let value = env::var(CORRUPT_SECRET_ENV).ok()?;
    value.parse().ok()
}

pub(crate) struct TestStubDeviceAdapter {
    devices: BTreeMap<u32, Rc<RefCell<TestStubDeviceState>>>,
    read_pin_from_tty: bool,
}

impl TestStubDeviceAdapter {
    pub(crate) fn production() -> Self {
        let default_state = parse_state_env(STUB_STATE_ENV).unwrap_or(StubState::Fresh);
        let primary_state = parse_state_env(PRIMARY_STUB_STATE_ENV).unwrap_or(default_state);
        let spare_state = parse_state_env(SPARE_STUB_STATE_ENV).unwrap_or(default_state);

        Self {
            devices: BTreeMap::from([
                (
                    2001,
                    Rc::new(RefCell::new(TestStubDeviceState::from_state(primary_state))),
                ),
                (
                    2002,
                    Rc::new(RefCell::new(TestStubDeviceState::from_state(spare_state))),
                ),
            ]),
            read_pin_from_tty: env::var(READ_PIN_FROM_TTY_ENV).as_deref() == Ok("true"),
        }
    }
}

impl DeviceSelectionPort for TestStubDeviceAdapter {
    type Device = TestStubSecretDevice;
    type DeviceCandidate = DiscoveredDevice;

    fn discover_devices(&mut self) -> Result<Vec<Self::DeviceCandidate>> {
        Ok(self
            .devices
            .keys()
            .map(|serial| DiscoveredDevice {
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
        })
    }
}

pub(crate) struct TestStubSecretDevice {
    serial: u32,
    state: Rc<RefCell<TestStubDeviceState>>,
    read_pin_from_tty: bool,
}

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

    #[allow(dead_code)]
    fn initialized() -> Self {
        let mut state = Self::fresh();
        state.key_exists = true;
        if let Ok(bytes) = encode_manifest(&SecretManifest::expected()) {
            state.objects.insert(PivObjectId::MANIFEST, bytes);
        }
        state
    }

    #[allow(dead_code)]
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
        state.objects.remove(&SecretName::BwsAccessToken.object_id());
        state
    }
}

fn make_seed_blob(name: SecretName, plaintext: &[u8]) -> Vec<u8> {
    let blob = SecretBlob {
        name,
        nonce: [0u8; NONCE_LEN],
        wrapped_key: vec![0u8; CONTENT_KEY_LEN],
        ciphertext: plaintext.to_vec(),
        tag: [0u8; TAG_LEN],
    };
    blob.encode().unwrap_or_else(|_| Vec::new())
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
        Ok(self.state.borrow().objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.state
            .borrow_mut()
            .objects
            .insert(object_id, value.to_vec());
        if let Some(name) = SecretName::iter().find(|name| name.object_id() == object_id) {
            let plaintext = SecretBlob::decode(value)
                .map(|blob| String::from_utf8_lossy(&blob.ciphertext).to_string())
                .unwrap_or_else(|_| "<decode-error>".to_owned());
            eprintln!(
                "{WRITE_EVENT_PREFIX} serial={} name={} value={}",
                self.serial, name, plaintext
            );
        }
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        Ok(key.to_vec())
    }

    fn requires_pin_input(&self) -> bool {
        self.read_pin_from_tty
    }

    fn verify_pin(&mut self, _pin: &[u8]) -> Result<()> {
        Ok(())
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(wrapped_key.to_vec()))
    }
}

impl SecretDeviceExt for TestStubSecretDevice {
    fn setup_storage(&mut self) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.key_exists = true;
        if !state.objects.contains_key(&PivObjectId::MANIFEST) {
            let mut manifest = encode_manifest(&SecretManifest::expected())?;
            state
                .objects
                .insert(PivObjectId::MANIFEST, std::mem::take(&mut manifest));
        }
        Ok(())
    }

    fn store_secret(
        &mut self,
        _random: &impl RandomBytesPort,
        name: SecretName,
        secret: &[u8],
        _force: bool,
    ) -> Result<()> {
        ensure_secret_value_non_empty(name, secret)?;
        self.setup_storage()?;
        self.state
            .borrow_mut()
            .objects
            .insert(name.object_id(), make_seed_blob(name, secret));
        eprintln!(
            "{WRITE_EVENT_PREFIX} serial={} name={} value={}",
            self.serial,
            name,
            String::from_utf8_lossy(secret)
        );
        Ok(())
    }

    fn load_secret(&mut self, name: SecretName) -> Result<Zeroizing<Vec<u8>>> {
        self.setup_storage()?;
        if corrupt_secret_name() == Some(name) {
            self.state
                .borrow_mut()
                .objects
                .insert(name.object_id(), b"not-json".to_vec());
        }
        if self.state.borrow().objects.get(&name.object_id()).is_none() && self.serial == 2001 {
            let seeded = seed_value_for(name);
            self.state
                .borrow_mut()
                .objects
                .insert(name.object_id(), make_seed_blob(name, seeded.as_slice()));
        }
        let encoded = self
            .state
            .borrow()
            .objects
            .get(&name.object_id())
            .cloned()
            .with_context(|| format!("{name} is not stored on this YubiKey"))?;
        let blob =
            SecretBlob::decode(&encoded).with_context(|| format!("failed to decode {name}"))?;
        ensure_secret_value_non_empty(name, blob.ciphertext.as_slice())?;
        Ok(Zeroizing::new(blob.ciphertext))
    }
}

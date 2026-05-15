//! `secrets-test-stub` feature でだけ使う CLI 統合テスト用 YubiKey 境界。
//!
//! この module は実プロセスの stdin/stdout/stderr をそのまま使い、YubiKey PIV 操作だけを
//! in-memory device に差し替える。通常 build には含めず、TTY / pipe の入力契約を binary
//! integration test で検証するための境界として閉じる。

use std::collections::{BTreeMap, VecDeque};

use zeroize::Zeroizing;

use super::{
    EnrollSpareOptions, SecretsBoundary, SecretsCommand, SecretsOptions, YubikeyCommand,
    application::{ProtectedBootstrapSecrets, ProtectedSecret, protect_secret_input},
    input::{read_hidden_secret, read_one_stdin_secret},
    storage::{self, SecretDevice, SecretName},
    util::{
        protection::{InterruptGuard, SecretMemoryGuard},
        terminal::{stdin_is_terminal, stdout_is_terminal},
    },
};
use crate::Result;

const DEFAULT_SERIAL: u32 = 2001;
const SPARE_SERIAL: u32 = 2002;

pub(super) struct TestSecretsBoundary {
    devices: VecDeque<TestDevice>,
}

impl TestSecretsBoundary {
    pub(super) fn for_options(options: &SecretsOptions) -> Result<Self> {
        Ok(Self {
            devices: devices_for_options(options)?.into(),
        })
    }
}

impl SecretsBoundary for TestSecretsBoundary {
    type Device = TestDevice;

    fn stdin_is_terminal(&self) -> bool {
        stdin_is_terminal()
    }

    fn stdout_is_terminal(&self) -> bool {
        stdout_is_terminal()
    }

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        let mut device = self
            .devices
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("test stub YubiKey queue is empty"))?;
        if let Some(serial) = serial {
            device.serial = serial;
        }
        Ok(device)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        _primary_serial: Option<u32>,
        _interrupt: &InterruptGuard,
    ) -> Result<Self::Device> {
        let mut device = self.open_device(spare_serial)?;
        if spare_serial.is_none() {
            device.serial = SPARE_SERIAL;
        }
        Ok(device)
    }

    fn read_bootstrap_secrets(
        &mut self,
        stdin_json: bool,
        memory: &SecretMemoryGuard,
    ) -> Result<ProtectedBootstrapSecrets> {
        super::read_protected_bootstrap_secrets(stdin_json, memory)
    }

    fn read_secret_for_put(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &SecretMemoryGuard,
    ) -> Result<ProtectedSecret> {
        let secret = if stdin {
            read_one_stdin_secret(super::MAX_SINGLE_STDIN_SECRET_LEN)?
        } else {
            read_hidden_secret(&format!("{}: ", name))?
        };
        protect_secret_input(secret, memory)
    }

    fn read_yubikey_pin(&mut self) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(b"123456".to_vec()))
    }

    fn prompt_yes_no(&mut self, prompt: &str) -> Result<bool> {
        super::util::terminal::prompt_yes_no(prompt)
    }
}

pub(super) struct TestDevice {
    serial: u32,
    key_exists: bool,
    objects: BTreeMap<storage::PivObjectId, Zeroizing<Vec<u8>>>,
}

impl TestDevice {
    fn fresh(serial: u32) -> Self {
        Self {
            serial,
            key_exists: false,
            objects: BTreeMap::new(),
        }
    }

    fn initialized(serial: u32) -> Result<Self> {
        let mut device = Self::fresh(serial);
        storage::setup(&mut device)?;
        Ok(device)
    }

    fn provisioned(serial: u32) -> Result<Self> {
        let mut device = Self::initialized(serial)?;
        storage::put(&mut device, SecretName::BwEmail, b"u@example.com", false)?;
        storage::put(&mut device, SecretName::BwPassword, b"pw", false)?;
        storage::put(&mut device, SecretName::BwsAccessToken, b"token", false)?;
        Ok(device)
    }

    fn writable_for(serial: u32, target: SecretName) -> Result<Self> {
        let mut device = Self::initialized(serial)?;
        for name in SecretName::iter().filter(|name| *name != target) {
            storage::put(&mut device, name, default_secret(name), false)?;
        }
        Ok(device)
    }
}

impl SecretDevice for TestDevice {
    fn serial(&self) -> u32 {
        self.serial
    }

    fn key_exists(&mut self) -> Result<bool> {
        Ok(self.key_exists)
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        self.key_exists = true;
        Ok(())
    }

    fn read_object(
        &mut self,
        object_id: storage::PivObjectId,
    ) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self.objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: storage::PivObjectId, value: &[u8]) -> Result<()> {
        self.objects
            .insert(object_id, Zeroizing::new(value.to_vec()));
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(key.iter().map(|byte| byte ^ 0xa5).collect()))
    }

    fn verify_pin(&mut self, _pin: &[u8]) -> Result<()> {
        Ok(())
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        self.wrap_key(wrapped_key)
    }
}

fn devices_for_options(options: &SecretsOptions) -> Result<Vec<TestDevice>> {
    match &options.command {
        SecretsCommand::Yubikey(options) => match &options.command {
            YubikeyCommand::Setup(options) => Ok(vec![TestDevice::fresh(
                options.serial.unwrap_or(DEFAULT_SERIAL),
            )]),
            YubikeyCommand::Put(options) => Ok(vec![TestDevice::writable_for(
                options.serial.unwrap_or(DEFAULT_SERIAL),
                options.name,
            )?]),
            YubikeyCommand::Get(options) => Ok(vec![TestDevice::provisioned(
                options.serial.unwrap_or(DEFAULT_SERIAL),
            )?]),
            YubikeyCommand::EnrollPrimary(options) => Ok(vec![TestDevice::fresh(
                options.serial.unwrap_or(DEFAULT_SERIAL),
            )]),
            YubikeyCommand::EnrollSpare(options) => devices_for_enroll_spare(options),
            YubikeyCommand::RotateBwsToken(options) => Ok(vec![TestDevice::provisioned(
                options.serial.unwrap_or(DEFAULT_SERIAL),
            )?]),
        },
        SecretsCommand::VerifyYubikey(options) => Ok(vec![TestDevice::provisioned(
            options.serial.unwrap_or(DEFAULT_SERIAL),
        )?]),
    }
}

fn devices_for_enroll_spare(options: &EnrollSpareOptions) -> Result<Vec<TestDevice>> {
    if options.stdin_json {
        return Ok(vec![TestDevice::fresh(
            options.spare_serial.unwrap_or(SPARE_SERIAL),
        )]);
    }

    if options.spare_serial.is_some() {
        return Ok(vec![
            TestDevice::fresh(options.spare_serial.unwrap_or(SPARE_SERIAL)),
            TestDevice::provisioned(options.primary_serial.unwrap_or(DEFAULT_SERIAL))?,
        ]);
    }

    Ok(vec![
        TestDevice::provisioned(options.primary_serial.unwrap_or(DEFAULT_SERIAL))?,
        TestDevice::fresh(SPARE_SERIAL),
    ])
}

fn default_secret(name: SecretName) -> &'static [u8] {
    match name {
        SecretName::BwEmail => b"u@example.com",
        SecretName::BwPassword => b"pw",
        SecretName::BwsAccessToken => b"token",
    }
}

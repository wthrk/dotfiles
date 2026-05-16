//! `secrets-test-stub` feature の CLI 統合テスト用 YubiKey 境界。
//!
//! 実プロセスの stdin/stdout/stderr を通し、YubiKey PIV 操作を in-memory device に
//! 差し替える。通常 build には含めず、TTY / pipe の入力契約を binary integration test
//! で検証する境界として閉じる。

use std::{collections::BTreeMap, io::Cursor, io::Write};

use clap::{Parser, ValueEnum};

#[path = "test_stub_contract.rs"]
mod test_stub_contract;

use super::{
    SecretsOptions,
    application::{
        MAX_SINGLE_STDIN_SECRET_LEN, ProtectedBootstrapSecrets, SecretsBoundary,
        read_protected_stdin_secret,
    },
    device::SPARE_SERIAL_NONINTERACTIVE_ERROR,
    input::{read_hidden_secret, read_yubikey_pin},
    storage::{self, SecretDevice, SecretName},
    util::{
        protection::{InterruptGuard, ProtectedInputBuffer, ProtectedSecret, SecretSession},
        terminal::{stdin_is_terminal, stdout_is_terminal},
    },
};
use crate::Result;
use test_stub_contract::{
    CORRUPT_SECRET_ENV, PRIMARY_SERIAL, PRIMARY_STUB_STATE_ENV, READ_PIN_FROM_TTY_ENV,
    SEED_BW_EMAIL_ENV, SEED_BW_PASSWORD_ENV, SEED_BWS_ACCESS_TOKEN_ENV, SPARE_SERIAL,
    SPARE_STUB_STATE_ENV, STUB_STATE_ENV, WRITE_EVENT_PREFIX,
};

const DEFAULT_SERIAL: u32 = PRIMARY_SERIAL;

#[derive(Clone, Default, Parser)]
#[command(name = "dotfiles-secrets-test-stub")]
/// device mock の初期状態と保存後検証条件を clap の env 経由で受け取る。
struct TestStubConfig {
    #[arg(long, env = STUB_STATE_ENV, value_enum)]
    state: Option<TestDeviceState>,
    #[arg(long, env = PRIMARY_STUB_STATE_ENV, value_enum)]
    state_2001: Option<TestDeviceState>,
    #[arg(long, env = SPARE_STUB_STATE_ENV, value_enum)]
    state_2002: Option<TestDeviceState>,
    #[arg(long, env = CORRUPT_SECRET_ENV, value_parser = parse_test_stub_secret_name)]
    corrupt_secret: Option<SecretName>,
    #[arg(long, env = READ_PIN_FROM_TTY_ENV)]
    read_pin_from_tty: bool,
    #[arg(long, env = SEED_BW_EMAIL_ENV)]
    seed_bw_email: Option<String>,
    #[arg(long, env = SEED_BW_PASSWORD_ENV)]
    seed_bw_password: Option<String>,
    #[arg(long, env = SEED_BWS_ACCESS_TOKEN_ENV)]
    seed_bws_access_token: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum TestDeviceState {
    Fresh,
    Initialized,
    Provisioned,
    WritableBwEmail,
    WritableBwPassword,
    WritableBwsAccessToken,
}

impl TestStubConfig {
    /// clap の env 解決を通して、テストプロセスから渡された device mock 条件を読む。
    fn from_env() -> Result<Self> {
        Ok(Self::try_parse_from(["dotfiles-secrets-test-stub"])?)
    }

    /// 保存済み状態を作るとき、clap が env から受けた値で既定値を置き換える。
    fn seed_secret(&self, name: SecretName) -> Vec<u8> {
        let value = match name {
            SecretName::BwEmail => self.seed_bw_email.as_deref(),
            SecretName::BwPassword => self.seed_bw_password.as_deref(),
            SecretName::BwsAccessToken => self.seed_bws_access_token.as_deref(),
        };
        value
            .map(|value| value.as_bytes().to_vec())
            .unwrap_or_else(|| match name {
                SecretName::BwEmail => b"u@example.com".to_vec(),
                SecretName::BwPassword => b"pw".to_vec(),
                SecretName::BwsAccessToken => b"token".to_vec(),
            })
    }

    fn state_for_serial(&self, serial: u32) -> TestDeviceState {
        match serial {
            PRIMARY_SERIAL => self.state_2001,
            SPARE_SERIAL => self.state_2002,
            _ => None,
        }
        .or(self.state)
        .unwrap_or(TestDeviceState::Fresh)
    }
}

fn parse_test_stub_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}

pub(super) struct TestSecretsBoundary {
    config: TestStubConfig,
    next_interactive_serial: u32,
}

impl TestSecretsBoundary {
    pub(super) fn for_options(options: &SecretsOptions) -> Result<Self> {
        let config = TestStubConfig::from_env()?;
        let _ = options;
        Ok(Self {
            config,
            next_interactive_serial: DEFAULT_SERIAL,
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
        if serial.is_none() && !stdin_is_terminal() {
            anyhow::bail!("pass --serial in non-interactive use");
        }

        let serial = serial.unwrap_or_else(|| {
            let serial = self.next_interactive_serial;
            self.next_interactive_serial = SPARE_SERIAL;
            serial
        });
        let mut device = TestDevice::from_config(serial, &self.config)?;
        device.emit_write_events = true;
        Ok(device)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        _interrupt: &InterruptGuard,
    ) -> Result<Self::Device> {
        if spare_serial.is_none() && !stdin_is_terminal() {
            anyhow::bail!(SPARE_SERIAL_NONINTERACTIVE_ERROR);
        }

        let serial = spare_serial.unwrap_or(SPARE_SERIAL);
        if primary_serial == Some(serial) {
            anyhow::bail!("primary and spare YubiKey serial must be different");
        }

        let mut device = TestDevice::from_config(serial, &self.config)?;
        device.emit_write_events = true;
        Ok(device)
    }

    fn read_bootstrap_secrets<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedBootstrapSecrets<'session>> {
        super::application::read_protected_bootstrap_secrets(stdin_json, memory)
    }

    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        if stdin {
            read_protected_stdin_secret(MAX_SINGLE_STDIN_SECRET_LEN, memory)
        } else {
            read_hidden_secret(&format!("{}: ", name), MAX_SINGLE_STDIN_SECRET_LEN, memory)
        }
    }

    fn read_yubikey_pin<'session>(
        &mut self,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        if self.config.read_pin_from_tty {
            return read_yubikey_pin(memory);
        }

        let input =
            ProtectedInputBuffer::read_from(Cursor::new(b"123456"), 6, "too large", memory)?;
        input.into_protected_secret(memory)
    }

    fn prompt_yes_no(&mut self, prompt: &str) -> Result<bool> {
        super::util::terminal::prompt_yes_no(prompt)
    }
}

pub(super) struct TestDevice {
    serial: u32,
    key_exists: bool,
    config: TestStubConfig,
    emit_write_events: bool,
    objects: BTreeMap<storage::PivObjectId, Vec<u8>>,
}

impl TestDevice {
    fn from_config(serial: u32, config: &TestStubConfig) -> Result<Self> {
        match config.state_for_serial(serial) {
            TestDeviceState::Fresh => Ok(Self::fresh(serial)),
            TestDeviceState::Initialized => Self::initialized(serial),
            TestDeviceState::Provisioned => Self::provisioned(serial, config),
            TestDeviceState::WritableBwEmail => {
                Self::writable_for(serial, SecretName::BwEmail, config)
            }
            TestDeviceState::WritableBwPassword => {
                Self::writable_for(serial, SecretName::BwPassword, config)
            }
            TestDeviceState::WritableBwsAccessToken => {
                Self::writable_for(serial, SecretName::BwsAccessToken, config)
            }
        }
    }

    fn fresh(serial: u32) -> Self {
        Self {
            serial,
            key_exists: false,
            config: TestStubConfig::default(),
            emit_write_events: false,
            objects: BTreeMap::new(),
        }
    }

    fn initialized(serial: u32) -> Result<Self> {
        let mut device = Self::fresh(serial);
        storage::setup(&mut device)?;
        Ok(device)
    }

    fn provisioned(serial: u32, config: &TestStubConfig) -> Result<Self> {
        let session = SecretSession::start()?;
        let mut device = Self::initialized(serial)?;
        device.config = config.clone();
        storage::put(
            &mut device,
            SecretName::BwEmail,
            &config.seed_secret(SecretName::BwEmail),
            false,
            &session,
        )?;
        storage::put(
            &mut device,
            SecretName::BwPassword,
            &config.seed_secret(SecretName::BwPassword),
            false,
            &session,
        )?;
        storage::put(
            &mut device,
            SecretName::BwsAccessToken,
            &config.seed_secret(SecretName::BwsAccessToken),
            false,
            &session,
        )?;
        if let Some(name) = config.corrupt_secret {
            device
                .objects
                .insert(name.object_id(), b"not-json".to_vec());
        }
        Ok(device)
    }

    fn writable_for(serial: u32, target: SecretName, config: &TestStubConfig) -> Result<Self> {
        let session = SecretSession::start()?;
        let mut device = Self::initialized(serial)?;
        device.config = config.clone();
        for name in SecretName::iter().filter(|name| *name != target) {
            storage::put(
                &mut device,
                name,
                &config.seed_secret(name),
                false,
                &session,
            )?;
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

    fn read_object(&mut self, object_id: storage::PivObjectId) -> Result<Option<Vec<u8>>> {
        Ok(self.objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: storage::PivObjectId, value: &[u8]) -> Result<()> {
        self.objects.insert(object_id, value.to_vec());
        self.emit_write_event(object_id)?;
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        Ok(key.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn verify_pin(&mut self, _pin: &[u8]) -> Result<()> {
        Ok(())
    }

    fn write_unwrapped_key(&mut self, wrapped_key: &[u8], output: &mut impl Write) -> Result<()> {
        output.write_all(&self.wrap_key(wrapped_key)?)?;
        Ok(())
    }
}

impl TestDevice {
    /// 保存直後に同じ device mock から復号し、CLI 統合テストが stderr で観測できる event を出す。
    fn emit_write_event(&mut self, object_id: storage::PivObjectId) -> Result<()> {
        if !self.emit_write_events {
            return Ok(());
        }
        let Some(name) = secret_name_for_object_id(object_id) else {
            return Ok(());
        };
        let session = SecretSession::start()?;
        let secret = storage::get_protected(self, name, &session)?;
        secret.with_secret(|value| {
            eprintln!(
                "{} serial={} name={} value={}",
                WRITE_EVENT_PREFIX,
                self.serial,
                name,
                String::from_utf8_lossy(value)
            );
        });
        Ok(())
    }
}

fn secret_name_for_object_id(object_id: storage::PivObjectId) -> Option<SecretName> {
    SecretName::iter().find(|name| name.object_id() == object_id)
}

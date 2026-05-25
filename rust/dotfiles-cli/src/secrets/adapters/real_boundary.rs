//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use anyhow::{bail, Context};
use zeroize::Zeroizing;

use super::{device_prompt, input, terminal, DeviceBackend};
use super::yubikey;
#[cfg(feature = "secrets-test-stub")]
use super::test_stub;
use crate::{
    secrets::{
        ports::{EnrollmentBytes, SecretsBoundary},
        support::protection::InterruptGuard,
    },
    Result,
};
#[cfg(feature = "secrets-test-stub")]
use crate::secrets::{domain::PivObjectId, ports::SecretDevice};

#[cfg(feature = "secrets-test-stub")]
/// 実機 YubiKey と device stub を同じ `SecretDevice` port として扱う adapter。
///
/// `secrets-test-stub` feature でだけ enum になり、application の use case は variant を見ない。
pub(super) enum YubikeySecretDevice {
    /// 実機 YubiKey の PIV device adapter。
    Real(yubikey::YubikeySecretDevice),
    /// CLI 統合テスト用の in-memory PIV device adapter。
    TestStub(test_stub::TestDevice),
}

#[cfg(not(feature = "secrets-test-stub"))]
/// 通常 build で application が扱う YubiKey device adapter。
pub(super) type YubikeySecretDevice = yubikey::YubikeySecretDevice;

#[cfg(feature = "secrets-test-stub")]
impl SecretDevice for YubikeySecretDevice {
    fn serial(&self) -> u32 {
        match self {
            Self::Real(device) => device.serial(),
            Self::TestStub(device) => device.serial(),
        }
    }

    fn key_exists(&mut self) -> Result<bool> {
        match self {
            Self::Real(device) => device.key_exists(),
            Self::TestStub(device) => device.key_exists(),
        }
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_key_generation_preconditions(),
            Self::TestStub(device) => device.check_key_generation_preconditions(),
        }
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_management_auth_preconditions(),
            Self::TestStub(device) => device.check_management_auth_preconditions(),
        }
    }

    fn generate_key(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.generate_key(),
            Self::TestStub(device) => device.generate_key(),
        }
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Real(device) => device.read_object(object_id),
            Self::TestStub(device) => device.read_object(object_id),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        match self {
            Self::Real(device) => device.write_object(object_id, value),
            Self::TestStub(device) => device.write_object(object_id, value),
        }
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Real(device) => device.wrap_key(key),
            Self::TestStub(device) => device.wrap_key(key),
        }
    }

    fn verify_pin(&mut self, pin: &[u8]) -> Result<()> {
        match self {
            Self::Real(device) => device.verify_pin(pin),
            Self::TestStub(device) => device.verify_pin(pin),
        }
    }

    fn requires_pin_input(&self) -> bool {
        match self {
            Self::Real(device) => device.requires_pin_input(),
            Self::TestStub(device) => device.requires_pin_input(),
        }
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        match self {
            Self::Real(device) => device.unwrap_key(wrapped_key),
            Self::TestStub(device) => device.unwrap_key(wrapped_key),
        }
    }
}

/// 実プロセスの stdin/stdout と device backend を接続する `SecretsBoundary` 実装。
pub(super) struct RealSecretsBoundary {
    backend: DeviceBackend,
}

impl RealSecretsBoundary {
    /// 指定した `DeviceBackend` で `RealSecretsBoundary` を構築する。
    pub(super) fn new(backend: DeviceBackend) -> Self {
        Self { backend }
    }
}

/// backend に対応する通常操作対象 device を開く。
///
/// 非対話時の serial 必須条件は実機 adapter の error contract にする。
fn open_device(backend: &mut DeviceBackend, serial: Option<u32>) -> Result<YubikeySecretDevice> {
    let io = device_prompt::yubikey_interaction();
    match backend {
        #[cfg(feature = "secrets-test-stub")]
        DeviceBackend::TestStub(factory) => factory
            .open_device(serial)
            .map(YubikeySecretDevice::TestStub),
        DeviceBackend::Real => {
            #[cfg(feature = "secrets-test-stub")]
            {
                yubikey::open_device(serial, &io).map(YubikeySecretDevice::Real)
            }
            #[cfg(not(feature = "secrets-test-stub"))]
            {
                yubikey::open_device(serial, &io)
            }
        }
    }
}

/// backend に対応する spare 登録対象 device を開く。
///
/// 実機 adapter では spare 待機の interrupt policy を適用する。
fn open_spare_device(
    backend: &mut DeviceBackend,
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    let io = device_prompt::yubikey_interaction();
    match backend {
        #[cfg(feature = "secrets-test-stub")]
        DeviceBackend::TestStub(factory) => factory
            .open_spare_device(spare_serial, primary_serial)
            .map(YubikeySecretDevice::TestStub),
        DeviceBackend::Real => {
            #[cfg(feature = "secrets-test-stub")]
            {
                yubikey::open_spare_device(spare_serial, primary_serial, interrupt, &io)
                    .map(YubikeySecretDevice::Real)
            }
            #[cfg(not(feature = "secrets-test-stub"))]
            {
                yubikey::open_spare_device(spare_serial, primary_serial, interrupt, &io)
            }
        }
    }
}

impl SecretsBoundary for RealSecretsBoundary {
    type Device = YubikeySecretDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        open_device(&mut self.backend, serial)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device> {
        let interrupt = InterruptGuard::install()
            .context("failed to install interrupt handler for spare YubiKey")?;
        open_spare_device(&mut self.backend, spare_serial, primary_serial, &interrupt)
    }

    fn require_serial(&self, serial: Option<u32>, error_message: &'static str) -> Result<()> {
        if serial.is_none() && !terminal::stdin_is_terminal() {
            bail!(error_message);
        }
        Ok(())
    }

    fn require_option(&self, enabled: bool, option_name: &'static str) -> Result<()> {
        if !enabled && !terminal::stdin_is_terminal() {
            bail!("pass {option_name} in non-interactive use");
        }
        Ok(())
    }

    fn require_stdin_pipe(&self) -> Result<()> {
        if terminal::stdin_is_terminal() {
            bail!("--stdin requires pipe or redirect input");
        }
        Ok(())
    }

    fn require_stdin_json_pipe(&self, enabled: bool) -> Result<()> {
        if enabled && terminal::stdin_is_terminal() {
            bail!("--stdin-json requires pipe or redirect input");
        }
        Ok(())
    }

    fn require_stdout_pipe(&self) -> Result<()> {
        if terminal::stdout_is_terminal() {
            bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
        }
        Ok(())
    }

    fn read_yubikey_pin_bytes(&self) -> Result<Zeroizing<Vec<u8>>> {
        let protected = input::read_yubikey_pin_raw()?;
        Ok(protected)
    }

    fn read_hidden_bytes(&self, prompt: &str, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        input::read_hidden_bytes(prompt, limit)
    }

    fn read_visible_line_bytes(&self, prompt: &str, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        input::read_visible_line_bytes(prompt, limit)
    }

    fn read_stdin_bytes(&self, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        input::read_stdin_bytes(limit)
    }

    fn read_enrollment_json_bytes(
        &self,
        input_limit: usize,
        field_limit: usize,
    ) -> Result<EnrollmentBytes> {
        input::read_enrollment_json_bytes(std::io::stdin(), input_limit, field_limit)
    }

    fn write_secret_to_stdout(&self, bytes: &[u8]) -> Result<()> {
        input::write_secret_to_stdout(bytes)
    }

    fn write_report(&self, value: &impl serde::Serialize) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }

    fn prompt_continue_rotation(&self) -> Result<bool> {
        terminal::prompt_yes_no(
            "Update another YubiKey? [y/N] ",
            &InterruptGuard::install()?,
        )
    }
}

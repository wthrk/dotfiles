//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use anyhow::{bail, Context};
use zeroize::Zeroizing;

use super::{input, open_device, open_spare_device, terminal, DeviceBackend, YubikeySecretDevice};
use crate::{
    secrets::{
        ports::{EnrollmentBytes, SecretsBoundary},
        support::protection::InterruptGuard,
    },
    Result,
};

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

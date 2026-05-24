//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use crate::secrets::{
    boundary::{EnrollmentSecretSet, SecretsBoundary},
    ports::SecretDevice,
};
use crate::{
    secrets::{
        adapters::input::{
            MAX_BOOTSTRAP_JSON_LEN, MAX_SINGLE_STDIN_SECRET_LEN, read_hidden_secret,
            read_protected_enrollment_secret_set, read_protected_stdin_secret, read_visible_secret_line,
            read_yubikey_pin, write_secret_to_stdout,
        },
        domain::SecretName,
        support::protection::{InterruptGuard, ProtectedSecret, SecretSession},
        adapters::terminal::{prompt_yes_no, stdin_is_terminal, stdout_is_terminal},
    },
    Result,
};
use anyhow::Context;
use std::io::Write;

/// 実プロセスの stdin/stdout と device backend を接続する `SecretsBoundary` 実装。
pub(in crate::secrets) struct RealSecretsBoundary {
    backend: crate::secrets::adapters::DeviceBackend,
}

impl RealSecretsBoundary {
    pub(in crate::secrets) fn new(backend: crate::secrets::adapters::DeviceBackend) -> Self {
        Self { backend }
    }
}

impl SecretsBoundary for RealSecretsBoundary {
    type Device = crate::secrets::adapters::YubikeySecretDevice;

    fn stdin_is_terminal(&self) -> bool {
        stdin_is_terminal()
    }

    fn stdout_is_terminal(&self) -> bool {
        stdout_is_terminal()
    }

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        crate::secrets::adapters::open_device(&mut self.backend, serial)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        interrupt: &InterruptGuard,
    ) -> Result<Self::Device> {
        crate::secrets::adapters::open_spare_device(&mut self.backend, spare_serial, primary_serial, interrupt)
    }

    fn read_enrollment_secret_set<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>> {
        if stdin_json {
            return read_protected_enrollment_secret_set(
                std::io::stdin(),
                MAX_BOOTSTRAP_JSON_LEN,
                MAX_SINGLE_STDIN_SECRET_LEN,
                memory,
            );
        }
        let bw_email = read_visible_secret_line("bw-email: ", MAX_SINGLE_STDIN_SECRET_LEN, memory)?;
        let bw_password = read_hidden_secret(
            &format!("{}: ", SecretName::BwPassword),
            MAX_SINGLE_STDIN_SECRET_LEN,
            memory,
        )?;
        let bws_access_token = read_hidden_secret(
            &format!("{}: ", SecretName::BwsAccessToken),
            MAX_SINGLE_STDIN_SECRET_LEN,
            memory,
        )?;
        Ok(EnrollmentSecretSet::new(
            bw_email,
            bw_password,
            bws_access_token,
        ))
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

    fn write_secret_output(&mut self, secret: &[u8]) -> Result<()> {
        write_secret_to_stdout(secret)
    }

    fn write_json_output<T: serde::Serialize>(&mut self, value: &T) -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, value)
            .context("failed to serialize JSON output")?;
        stdout.write_all(b"\n")?;
        Ok(())
    }

    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool> {
        prompt_yes_no(prompt, interrupt)
    }

    fn device_serial(&self, device: &Self::Device) -> u32 {
        device.serial()
    }

    fn verify_pin_for_secret_reads(
        &mut self,
        device: &mut Self::Device,
        session: &SecretSession,
    ) -> Result<()> {
        if !device.requires_pin_input() {
            return Ok(());
        }
        let pin = read_yubikey_pin(session)?;
        pin.with_secret(|pin| session.run_yubikey_operation(|| device.verify_pin(pin)))
    }

    fn check_management_auth_preconditions(
        &mut self,
        device: &mut Self::Device,
        session: &SecretSession,
    ) -> Result<()> {
        session.run_yubikey_operation(|| device.check_management_auth_preconditions())
    }
}

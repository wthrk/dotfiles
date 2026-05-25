//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use anyhow::Context;

use super::{input, open_device, open_spare_device, terminal, DeviceBackend, YubikeySecretDevice};
use crate::{
    secrets::{
        ports::SecretsBoundary,
        support::protection::{InterruptGuard, ProtectedSecret, SecretSession},
        EnrollmentSecretSet,
    },
    Result,
};

/// 実プロセスの stdin/stdout と device backend を接続する `SecretsBoundary` 実装。
pub(crate) struct RealSecretsBoundary {
    pub(crate) backend: DeviceBackend,
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

    fn stdin_is_terminal(&self) -> bool {
        terminal::stdin_is_terminal()
    }

    fn stdout_is_terminal(&self) -> bool {
        terminal::stdout_is_terminal()
    }

    fn read_yubikey_pin<'session>(
        &self,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        input::read_yubikey_pin(session)
    }

    fn read_hidden_secret<'session>(
        &self,
        prompt: &str,
        limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        input::read_hidden_secret(prompt, limit, session)
    }

    fn read_visible_secret_line<'session>(
        &self,
        prompt: &str,
        limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        input::read_visible_secret_line(prompt, limit, session)
    }

    fn read_protected_stdin_secret<'session>(
        &self,
        limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        input::read_protected_stdin_secret(limit, session)
    }

    fn read_protected_enrollment_secret_set<'session>(
        &self,
        input_limit: usize,
        field_limit: usize,
        session: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>> {
        input::read_protected_enrollment_secret_set(
            std::io::stdin(),
            input_limit,
            field_limit,
            session,
        )
    }

    fn write_secret_to_stdout(&self, bytes: &[u8]) -> Result<()> {
        input::write_secret_to_stdout(bytes)
    }

    fn write_report(&self, value: &impl serde::Serialize) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }

    fn prompt_yes_no(&self, prompt: &str, session: &SecretSession) -> Result<bool> {
        terminal::prompt_yes_no(prompt, session.interrupt())
    }
}

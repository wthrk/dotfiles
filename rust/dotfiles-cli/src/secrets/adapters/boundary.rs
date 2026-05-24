//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use crate::{
    secrets::{
        adapters::{self, input::read_yubikey_pin, terminal},
        application::{EnrollmentSecretSet, SecretsBoundary, read_enrollment_secret_set_from_user, read_protected_secret_for_put},
        domain::SecretName,
        support::protection::{InterruptGuard, ProtectedSecret, SecretSession},
    },
    Result,
};

/// 実プロセスの stdin/stdout と device backend を接続する `SecretsBoundary` 実装。
pub(crate) struct RealSecretsBoundary {
    pub(crate) backend: adapters::DeviceBackend,
}

impl SecretsBoundary for RealSecretsBoundary {
    type Device = adapters::YubikeySecretDevice;

    fn stdin_is_terminal(&self) -> bool {
        terminal::stdin_is_terminal()
    }

    fn stdout_is_terminal(&self) -> bool {
        terminal::stdout_is_terminal()
    }

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        adapters::open_device(&mut self.backend, serial)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        interrupt: &InterruptGuard,
    ) -> Result<Self::Device> {
        adapters::open_spare_device(&mut self.backend, spare_serial, primary_serial, interrupt)
    }

    fn read_enrollment_secret_set<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>> {
        read_enrollment_secret_set_from_user(stdin_json, memory)
    }

    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        read_protected_secret_for_put(name, stdin, memory)
    }

    fn read_yubikey_pin<'session>(
        &mut self,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        read_yubikey_pin(memory)
    }

    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool> {
        terminal::prompt_yes_no(prompt, interrupt)
    }
}

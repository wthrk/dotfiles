//! 実プロセス I/O と実機/stub backend を port 群へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use super::{EnrollmentSecretSet, adapters};
use crate::{
    Result,
    secrets::{
        adapters::input,
        domain::SecretName,
        ports::{
            device::SecretDeviceFactoryPort,
            io::{SecretInputPort, SecretOutputPort, TerminalEnvironmentPort, TerminalPromptPort},
        },
        support::{
            protection::{InterruptGuard, ProtectedSecret, SecretSession},
            terminal::{prompt_yes_no, stdin_is_terminal},
        },
    },
};

/// 実プロセスの stdin/stdout と device backend を接続する port adapter。
pub(super) struct RealSecretsBoundary {
    pub(super) backend: adapters::DeviceBackend,
}

impl TerminalEnvironmentPort for RealSecretsBoundary {
    fn stdin_is_terminal(&self) -> bool {
        stdin_is_terminal()
    }
}

impl SecretDeviceFactoryPort for RealSecretsBoundary {
    type Device = adapters::YubikeySecretDevice;

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
}

impl SecretInputPort for RealSecretsBoundary {
    fn read_enrollment_secret_set<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>> {
        input::read_enrollment_secret_set_from_user(stdin_json, memory)
    }

    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        input::read_secret_for_put(name, stdin, memory)
    }

    fn read_yubikey_pin<'session>(
        &mut self,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        input::read_yubikey_pin(memory)
    }
}

impl TerminalPromptPort for RealSecretsBoundary {
    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool> {
        prompt_yes_no(prompt, interrupt)
    }
}

impl SecretOutputPort for RealSecretsBoundary {
    fn ensure_secret_stdout_target(&self) -> Result<()> {
        input::ensure_secret_stdout_not_terminal()
    }

    fn write_secret_output(&mut self, bytes: &[u8]) -> Result<()> {
        input::write_secret_to_stdout(bytes)
    }
}

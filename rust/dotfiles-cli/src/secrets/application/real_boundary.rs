//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use super::adapters;
use crate::{
    secrets::{
        adapters::input::{
            MAX_BOOTSTRAP_JSON_LEN, MAX_SINGLE_STDIN_SECRET_LEN, read_hidden_secret,
            read_protected_enrollment_secret_set, read_protected_stdin_secret, read_visible_secret_line,
            read_yubikey_pin, reject_secret_stdout_terminal, write_secret_to_stdout,
        },
        domain::SecretName,
        ports::{EnrollmentSecretSet, SecretsBoundary},
        support::{
            protection::{InterruptGuard, ProtectedSecret, SecretSession},
            terminal::{prompt_yes_no, stdin_is_terminal, write_all_stdout},
        },
    },
    Result,
};

/// 実プロセスの stdin/stdout と device backend を接続する `SecretsBoundary` 実装。
pub(super) struct RealSecretsBoundary {
    pub(super) backend: adapters::DeviceBackend,
}

impl SecretsBoundary for RealSecretsBoundary {
    type Device = adapters::YubikeySecretDevice;

    fn stdin_is_terminal(&self) -> bool {
        stdin_is_terminal()
    }

    fn stdout_is_terminal(&self) -> bool {
        super::super::support::terminal::stdout_is_terminal()
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
        if stdin_json {
            return read_protected_enrollment_secret_set(
                std::io::stdin(),
                MAX_BOOTSTRAP_JSON_LEN,
                MAX_SINGLE_STDIN_SECRET_LEN,
                memory,
            );
        }
        let bw_email = read_visible_secret_line("bw-email: ", MAX_SINGLE_STDIN_SECRET_LEN, memory)?;
        let bw_password =
            read_hidden_secret("bw-password: ", MAX_SINGLE_STDIN_SECRET_LEN, memory)?;
        let bws_access_token =
            read_hidden_secret("bws-access-token: ", MAX_SINGLE_STDIN_SECRET_LEN, memory)?;
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

    fn read_yubikey_pin<'session>(
        &mut self,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        read_yubikey_pin(memory)
    }

    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool> {
        prompt_yes_no(prompt, interrupt)
    }

    fn write_secret_to_stdout(&mut self, bytes: &[u8]) -> Result<()> {
        write_secret_to_stdout(bytes)
    }

    fn write_json_line(&mut self, line: &str) -> Result<()> {
        write_all_stdout(line.as_bytes())?;
        write_all_stdout(b"\n")
    }

    fn reject_secret_stdout_terminal(&self) -> Result<()> {
        reject_secret_stdout_terminal()
    }
}

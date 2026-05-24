//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use crate::{
    Result,
    secrets::{
        adapters::{
            self,
            input::{
                MAX_BOOTSTRAP_JSON_LEN, MAX_SINGLE_STDIN_SECRET_LEN,
                ensure_secret_stdout_not_terminal, read_hidden_secret,
                read_protected_enrollment_secret_set, read_protected_stdin_secret,
                read_visible_secret_line, read_yubikey_pin, write_secret_to_stdout,
            },
            terminal,
        },
        application::{EnrollmentSecretSet, InteractionBoundary},
        domain::SecretName,
        ports::{SecretDevice, SecretsBoundary},
        support::protection::{InterruptGuard, ProtectedSecret, SecretSession},
    },
};

/// 実プロセスの stdin/stdout と device backend を接続する `SecretsBoundary` 実装。
pub(crate) struct RealSecretsBoundary {
    pub(crate) backend: adapters::DeviceBackend,
}

impl SecretsBoundary for RealSecretsBoundary {
    type Device = adapters::YubikeySecretDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        adapters::open_device(&mut self.backend, serial)
    }
}

impl InteractionBoundary for RealSecretsBoundary {
    fn stdin_is_terminal(&self) -> bool {
        terminal::stdin_is_terminal()
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
            let secrets = read_protected_enrollment_secret_set(
                std::io::stdin(),
                MAX_BOOTSTRAP_JSON_LEN,
                MAX_SINGLE_STDIN_SECRET_LEN,
                memory,
            )?;
            return Ok(EnrollmentSecretSet::new(
                secrets.bw_email,
                secrets.bw_password,
                secrets.bws_access_token,
            ));
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

    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool> {
        terminal::prompt_yes_no(prompt, interrupt)
    }

    fn write_summary_json(&mut self, summary: &impl serde::Serialize) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(summary)?);
        Ok(())
    }

    fn write_secret_to_stdout(&mut self, bytes: &[u8]) -> Result<()> {
        write_secret_to_stdout(bytes)
    }

    fn ensure_secret_stdout_not_terminal(&self) -> Result<()> {
        ensure_secret_stdout_not_terminal()
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

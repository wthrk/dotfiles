//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。

use super::{DeviceBackend, YubikeySecretDevice, input, open_device, open_spare_device, terminal};
use crate::{
    Result,
    secrets::{domain::SecretName, ports::SecretsBoundary},
};

pub(crate) struct RealSecretsBoundary {
    pub(crate) backend: DeviceBackend,
}

impl SecretsBoundary for RealSecretsBoundary {
    type Device = YubikeySecretDevice;

    fn stdin_is_terminal(&self) -> bool {
        terminal::stdin_is_terminal()
    }
    fn stdout_is_terminal(&self) -> bool {
        terminal::stdout_is_terminal()
    }
    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        open_device(&mut self.backend, serial)
    }
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device> {
        let interrupt = crate::secrets::support::protection::InterruptGuard::install()?;
        open_spare_device(&mut self.backend, spare_serial, primary_serial, &interrupt)
    }
    fn read_enrollment_secret_set(
        &mut self,
        stdin_json: bool,
    ) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        if stdin_json {
            return input::read_enrollment_secret_set_from_json(std::io::stdin());
        }
        let bw_email = input::read_visible_secret_line("bw-email: ")?;
        let bw_password = input::read_hidden_secret(&format!("{}: ", SecretName::BwPassword))?;
        let bws_access_token =
            input::read_hidden_secret(&format!("{}: ", SecretName::BwsAccessToken))?;
        Ok((bw_email, bw_password, bws_access_token))
    }
    fn read_secret_for_put(&mut self, name: SecretName, stdin: bool) -> Result<Vec<u8>> {
        if stdin {
            input::read_protected_stdin_secret()
        } else {
            input::read_hidden_secret(&format!("{}: ", name))
        }
    }
    fn read_yubikey_pin(&mut self) -> Result<Vec<u8>> {
        input::read_yubikey_pin()
    }
    fn confirm_update_another_yubikey(&mut self) -> Result<bool> {
        let interrupt = crate::secrets::support::protection::InterruptGuard::install()?;
        terminal::prompt_yes_no("Update another YubiKey? [y/N] ", &interrupt)
    }
    fn write_secret_output(&mut self, bytes: &[u8]) -> Result<()> {
        input::write_secret_to_stdout(bytes)
    }
    fn write_json_report(&mut self, value: &impl serde::Serialize) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }
}

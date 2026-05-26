//! 実プロセスの secret 入出力を `SecretInputPort` / `SecretOutputPort` 系契約へ翻訳する adapter。

use anyhow::bail;
use zeroize::Zeroizing;

use crate::{
    Result,
    secrets::{
        domain::{
            BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT, PIV_PIN_MAX_LEN, PIV_PIN_MIN_LEN,
            decode_bootstrap_secret_document,
        },
        ports::{PinInputPort, SecretInputPort, SecretOutputPort},
    },
};

use super::console_io::{self, SECRET_STDOUT_TERMINAL_ERROR};

/// `dotfiles secrets` の標準入出力境界を担う runtime adapter。
pub(super) struct RealSecretIoAdapter;

impl PinInputPort for RealSecretIoAdapter {
    fn read_pin(&self) -> Result<Zeroizing<Vec<u8>>> {
        let pin = console_io::read_hidden_line_bytes(
            "YubiKey PIN: ",
            PIV_PIN_MAX_LEN,
            "YubiKey PIN is too long",
        )?;
        if !(PIV_PIN_MIN_LEN..=PIV_PIN_MAX_LEN).contains(&pin.len()) {
            bail!("YubiKey PIN must be 6 to 8 bytes");
        }
        Ok(pin)
    }
}

impl SecretInputPort for RealSecretIoAdapter {
    fn read_visible_secret(&self, label: &str) -> Result<Zeroizing<Vec<u8>>> {
        console_io::read_visible_line_bytes(label, 16 * 1024, "visible secret input is too large")
    }

    fn read_hidden_secret(&self, label: &str) -> Result<Zeroizing<Vec<u8>>> {
        console_io::read_hidden_line_bytes(label, 16 * 1024, "hidden secret input is too large")
    }

    fn read_stdin_secret(&self) -> Result<Zeroizing<Vec<u8>>> {
        console_io::read_stdin_line_bytes(
            16 * 1024,
            "--stdin requires pipe or redirect input",
            "stdin secret input is too large",
        )
    }

    fn read_secret_document_noninteractive(&self) -> Result<Zeroizing<Vec<u8>>> {
        console_io::read_stdin_all_bytes(
            64 * 1024,
            "--stdin-json requires pipe or redirect input",
            "bootstrap secret JSON input is too large",
        )
    }
    fn read_bootstrap_secret_document(
        &self,
    ) -> Result<crate::secrets::domain::BootstrapSecretDocument> {
        let bytes = self.read_secret_document_noninteractive()?;
        decode_bootstrap_secret_document(bytes.as_ref(), BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT)
    }
}

impl SecretOutputPort for RealSecretIoAdapter {
    fn write_secret(&self, bytes: &[u8]) -> Result<()> {
        console_io::write_stdout_bytes_if_not_terminal(bytes, SECRET_STDOUT_TERMINAL_ERROR)
    }
}

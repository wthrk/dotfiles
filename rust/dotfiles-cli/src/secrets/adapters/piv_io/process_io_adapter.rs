//! 端末/標準入力の secret I/O を process helper と port 契約の間で翻訳する adapter。
//!
//! prompt 文言や入力上限はこの境界に閉じ、use case 手順や storage 判定は扱わない。

use std::collections::BTreeMap;

use crate::{
    Result,
    secrets::{
        domain::{manifest::BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT, material::SecretMaterial},
        ports::{
            BootstrapSecretDocumentInputPort, PinInputPort, RotationContinuationPort,
            SecretInputPort, SecretOutputPort,
        },
        support::process_io,
    },
};

use super::{material_from_protected, protected_from_material};

#[derive(Default)]
struct RealSecretIoAdapter;

impl PinInputPort for RealSecretIoAdapter {
    fn read_pin(&self) -> Result<SecretMaterial> {
        let protected = process_io::read_hidden_line(
            "YubiKey PIN: ",
            crate::secrets::domain::piv::PIV_PIN_MAX_LEN,
            "YubiKey PIN is too long",
        )?;
        Ok(material_from_protected(protected))
    }
}

impl SecretInputPort for RealSecretIoAdapter {
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        process_io::read_visible_line("bw-email: ", 16 * 1024, "visible secret input is too large")
            .map(material_from_protected)
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        process_io::read_hidden_line(
            "bw-password: ",
            16 * 1024,
            "hidden secret input is too large",
        )
        .map(material_from_protected)
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        process_io::read_hidden_line(
            "bws-access-token: ",
            16 * 1024,
            "hidden secret input is too large",
        )
        .map(material_from_protected)
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        let protected = process_io::read_stdin_line(16 * 1024, "stdin secret input is too large")?;
        Ok(material_from_protected(protected))
    }
}

impl RotationContinuationPort for RealSecretIoAdapter {
    fn continue_rotation(&self) -> Result<bool> {
        if !process_io::stdin_is_terminal() {
            return Ok(false);
        }
        let answer = process_io::read_control_line("rotate another YubiKey? [y/N]: ")?;
        Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
    }
}

impl BootstrapSecretDocumentInputPort for RealSecretIoAdapter {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        let protected =
            process_io::read_stdin_all(64 * 1024, "bootstrap secret JSON input is too large")?;
        let fields = protected.decode_json_string_map(BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT)?;
        Ok(fields
            .into_iter()
            .map(|(name, secret)| (name, material_from_protected(secret)))
            .collect())
    }
}

impl SecretOutputPort for RealSecretIoAdapter {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        process_io::write_secret_stdout(protected_from_material(secret)?)
    }
}

#[derive(Default)]
pub(crate) struct ProcessIoAdapter {
    secret_io: RealSecretIoAdapter,
}

impl PinInputPort for ProcessIoAdapter {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.secret_io.read_pin()
    }
}

impl SecretInputPort for ProcessIoAdapter {
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_bw_email_secret()
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_bw_password_secret()
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_bws_access_token_secret()
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_streamed_secret()
    }
}

impl RotationContinuationPort for ProcessIoAdapter {
    fn continue_rotation(&self) -> Result<bool> {
        self.secret_io.continue_rotation()
    }
}

impl BootstrapSecretDocumentInputPort for ProcessIoAdapter {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        self.secret_io.read_bootstrap_secret_fields()
    }
}

impl SecretOutputPort for ProcessIoAdapter {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.secret_io.write_secret(secret)
    }
}

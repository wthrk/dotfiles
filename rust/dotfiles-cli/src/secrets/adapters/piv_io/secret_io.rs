//! `support::process_io` を port 契約へ接続する薄い adapter。

use anyhow::bail;

use crate::{
    Result,
    secrets::{
        domain::{
            manifest::{BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT, BootstrapSecretDocument},
            material::SecretMaterial,
            piv::{PIV_PIN_MAX_LEN, PIV_PIN_MIN_LEN},
        },
        ports::{
            BootstrapSecretDocumentInputPort, PinInputPort, SecretInputPort, SecretOutputPort,
        },
        support::process_io,
    },
};

/// `dotfiles secrets` の標準入出力境界を担う runtime adapter。
pub(super) struct RealSecretIoAdapter;

impl PinInputPort for RealSecretIoAdapter {
    fn read_pin(&self) -> Result<SecretMaterial> {
        let protected = process_io::read_hidden_line(
            "YubiKey PIN: ",
            PIV_PIN_MAX_LEN,
            "YubiKey PIN is too long",
        )?;
        let pin = protected;
        if !(PIV_PIN_MIN_LEN..=PIV_PIN_MAX_LEN).contains(&pin.len()) {
            bail!("YubiKey PIN must be 6 to 8 bytes");
        }
        Ok(pin)
    }
}

impl SecretInputPort for RealSecretIoAdapter {
    fn read_visible_secret(&self) -> Result<SecretMaterial> {
        let protected = process_io::read_visible_line(
            "bw-email: ",
            16 * 1024,
            "visible secret input is too large",
        )?;
        Ok(protected)
    }

    fn read_hidden_secret(
        &self,
        name: crate::secrets::domain::piv::SecretName,
    ) -> Result<SecretMaterial> {
        let prompt = format!("{name}: ");
        let protected =
            process_io::read_hidden_line(&prompt, 16 * 1024, "hidden secret input is too large")?;
        Ok(protected)
    }

    fn read_stdin_secret(&self) -> Result<SecretMaterial> {
        let protected = process_io::read_stdin_line(16 * 1024, "stdin secret input is too large")?;
        Ok(protected)
    }
}

impl BootstrapSecretDocumentInputPort for RealSecretIoAdapter {
    fn read_bootstrap_secret_document_noninteractive(&self) -> Result<BootstrapSecretDocument> {
        let protected =
            process_io::read_stdin_all(64 * 1024, "bootstrap secret JSON input is too large")?;
        let mut fields = protected.decode_json_string_map(BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT)?;
        let missing = |field: &str| anyhow::anyhow!("JSON field `{field}` is missing");
        let bw_email = fields
            .remove("bw-email")
            .ok_or_else(|| missing("bw-email"))?;
        let bw_password = fields
            .remove("bw-password")
            .ok_or_else(|| missing("bw-password"))?;
        let bws_access_token = fields
            .remove("bws-access-token")
            .ok_or_else(|| missing("bws-access-token"))?;

        BootstrapSecretDocument::from_secret_materials(&bw_email, &bw_password, &bws_access_token)
    }
}

impl SecretOutputPort for RealSecretIoAdapter {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        process_io::write_secret_stdout(secret)
    }
}

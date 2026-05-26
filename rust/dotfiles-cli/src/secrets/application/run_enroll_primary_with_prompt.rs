use crate::Result;
use crate::secrets::{
    domain::{BootstrapSecretDocument, EnrollPrimaryCommand, SecretName},
    ports::{self},
};

/// prompt 入力で primary YubiKey に bootstrap secret 一式を登録する。
pub(crate) fn run_enroll_primary_with_prompt<
    B: ports::DeviceSerialPort
        + ports::StorageSetupPort
        + ports::SecretInputPort
        + ports::BootstrapSecretStorePort
        + ports::StorageVerifyPort
        + ports::ReportPort,
>(
    command: EnrollPrimaryCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    boundary.setup_storage(serial)?;
    let bw_email = read_secret_for_name(boundary, SecretName::BwEmail)?;
    let bw_password = read_secret_for_name(boundary, SecretName::BwPassword)?;
    let bws_access_token = read_secret_for_name(boundary, SecretName::BwsAccessToken)?;
    let document = BootstrapSecretDocument::from_interactive_secrets(
        bw_email.as_ref(),
        bw_password.as_ref(),
        bws_access_token.as_ref(),
    )?;
    boundary.store_bootstrap_secret_document(serial, &document)?;
    boundary.verify_local_storage(serial)?;
    boundary.report_primary_enrollment(serial)
}

fn read_secret_for_name<B: ports::SecretInputPort>(
    boundary: &B,
    name: SecretName,
) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    let label = format!("{name}: ");
    if name.uses_visible_input() {
        boundary.read_visible_secret(&label)
    } else {
        boundary.read_hidden_secret(&label)
    }
}

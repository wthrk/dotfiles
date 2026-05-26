use crate::Result;
use crate::secrets::{
    domain::EnrollPrimaryCommand,
    ports::{self},
};

/// stdin JSON document で primary YubiKey に bootstrap secret 一式を登録する。
pub(crate) fn run_enroll_primary_with_stdin_json<
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
    let document = boundary.read_bootstrap_secret_document()?;
    boundary.store_bootstrap_secret_document(serial, &document)?;
    boundary.verify_local_storage(serial)?;
    boundary.report_primary_enrollment(serial)
}

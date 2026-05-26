use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::EnrollSpareCommand,
    ports::{self},
};

/// stdin JSON document で spare YubiKey に bootstrap secret 一式を登録する。
pub(crate) fn run_enroll_spare_with_stdin_json<
    B: ports::SpareDeviceSerialPort
        + ports::StorageSetupPort
        + ports::SecretInputPort
        + ports::BootstrapSecretStorePort
        + ports::StorageVerifyPort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let spare_serial =
        boundary.resolve_spare_device_serial(command.primary_serial, command.spare_serial)?;
    if command.primary_serial == Some(spare_serial) {
        bail!("primary and spare YubiKey serial must be different");
    }
    boundary.setup_storage(spare_serial)?;
    let document = boundary.read_bootstrap_secret_document()?;
    boundary.store_bootstrap_secret_document(spare_serial, &document)?;
    boundary.verify_local_storage(spare_serial)?;
    boundary.report_spare_enrollment(spare_serial)
}

use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::EnrollSpareCommand,
    ports::{self},
};

/// primary YubiKey から読み出した secret を prompt 運用の spare YubiKey へ複製する。
pub(crate) fn run_enroll_spare_with_prompt<
    B: ports::DeviceSerialPort
        + ports::SpareDeviceSerialPort
        + ports::StorageSetupPort
        + ports::BootstrapSecretLoadPort
        + ports::BootstrapSecretStorePort
        + ports::StorageVerifyPort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let primary_serial = boundary.resolve_device_serial(command.primary_serial)?;
    let spare_serial =
        boundary.resolve_spare_device_serial(Some(primary_serial), command.spare_serial)?;
    if spare_serial == primary_serial {
        bail!("primary and spare YubiKey serial must be different");
    }
    boundary.setup_storage(spare_serial)?;
    let document = boundary.load_bootstrap_secret_document(primary_serial)?;
    boundary.store_bootstrap_secret_document(spare_serial, &document)?;
    boundary.verify_local_storage(spare_serial)?;
    boundary.report_spare_enrollment(spare_serial)
}

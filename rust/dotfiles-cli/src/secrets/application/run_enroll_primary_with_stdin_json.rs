use crate::Result;
use crate::secrets::{
    domain::values::{EnrollPrimaryCommand, EnrollSummary},
    ports::{self, SecretStoragePort},
};

/// stdin JSON document で primary YubiKey に bootstrap secret 一式を登録する。
///
/// JSON parse を use case へ持ち込まず `BootstrapSecretDocumentInputPort` へ委譲し、
/// enrollment 手順のみを application 層で固定する。
pub(crate) fn run_enroll_primary_with_stdin_json<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::BootstrapSecretDocumentInputPort
        + SecretStoragePort
        + ports::ReportPort,
>(
    command: EnrollPrimaryCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    boundary.initialize_secret_storage(serial)?;
    let document = boundary.read_bootstrap_secret_document_noninteractive()?;
    for (storage, value) in document.storage_entries(serial) {
        boundary.store_secret(serial, storage, value)?;
    }
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    boundary.verify_local_storage(serial, pin.as_ref())?;
    boundary.write_enroll_report(&EnrollSummary::primary_completed(serial))
}

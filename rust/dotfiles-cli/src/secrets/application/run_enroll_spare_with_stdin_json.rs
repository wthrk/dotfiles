use crate::Result;
use crate::secrets::{
    domain::values::{EnrollSpareCommand, EnrollSummary},
    ports::{self, SecretStoragePort},
};

/// stdin JSON document で spare YubiKey に bootstrap secret 一式を登録する。
///
/// primary と spare の衝突停止条件を先に評価し、device 選択・入力実装は port に委譲して
/// use case の順序責務だけを保持する。
pub(crate) fn run_enroll_spare_with_stdin_json<
    B: ports::SpareDeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::BootstrapSecretDocumentInputPort
        + SecretStoragePort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let spare_serial = boundary.resolve_spare_device_serial(command.spare_serial)?;
    command.ensure_requested_primary_differs_from_spare(spare_serial)?;
    boundary.initialize_secret_storage(spare_serial)?;
    let document = boundary.read_bootstrap_secret_document_noninteractive()?;
    for (storage, value) in document.storage_entries(spare_serial) {
        boundary.store_secret(spare_serial, storage, value)?;
    }
    let pin = if boundary.device_requires_pin(spare_serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    boundary.verify_local_storage(spare_serial, pin.as_ref())?;
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

use crate::Result;
use crate::secrets::{
    domain::{
        piv::SecretStorageSpec,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageWriteIntent,
        },
        values::{EnrollPrimaryCommand, EnrollSummary},
    },
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
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = boundary.inspect_secret_storage_setup(serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::from_inspection(setup_inspection)?;
    boundary.initialize_secret_storage(serial, setup_intent)?;
    let document = boundary.read_bootstrap_secret_document()?;
    for (storage, value) in document.storage_entries(serial) {
        let inspection = boundary.inspect_secret_storage_write(serial, &storage)?;
        let intent = SecretStorageWriteIntent::store(storage, inspection)?;
        boundary.store_secret(serial, intent, value)?;
    }
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    for storage in SecretStorageSpec::all_for_serial(serial) {
        let inspection = boundary.inspect_secret_storage_read(serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let _secret = boundary.load_secret(serial, intent, pin.as_ref())?;
    }
    boundary.write_enroll_report(&EnrollSummary::primary_completed(serial))
}

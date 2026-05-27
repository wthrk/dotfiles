use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
        piv::validate_piv_pin_len,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
        values::{EnrollSpareCommand, EnrollSummary},
    },
    ports::{self, SecretStoragePort},
};

/// primary YubiKey から読み出した secret を prompt 運用の spare YubiKey へ複製する。
///
/// primary/spare 解決順序を固定して同一 serial への誤登録を防ぎ、secret 転送手段の詳細は
/// port 境界で読み出しと保存を接続する。
pub(crate) fn run_enroll_spare_with_prompt<
    B: ports::DeviceSerialPort
        + ports::SpareDeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let primary_serial = boundary.resolve_device_serial(command.primary_serial)?;
    let spare_serial = boundary.resolve_spare_device_serial(command.spare_serial)?;
    command.ensure_distinct_resolved_serials(primary_serial, spare_serial)?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = boundary.inspect_secret_storage_setup(spare_serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::from_inspection(setup_inspection)?;
    boundary.initialize_secret_storage(spare_serial, setup_intent)?;
    let primary_pin = if boundary.device_requires_pin(primary_serial)? {
        let pin = boundary.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let [first_storage, second_storage, third_storage] =
        SecretStorageVerificationPlan::for_serial(primary_serial).into_targets();
    let first_inspection = boundary.inspect_secret_storage_read(primary_serial, &first_storage)?;
    let first_intent = SecretStorageReadIntent::from_inspection(first_storage, first_inspection)?;
    let first_document_storage = first_intent.storage.clone();
    let first = boundary.load_secret(primary_serial, first_intent, primary_pin.as_ref())?;
    let second_inspection =
        boundary.inspect_secret_storage_read(primary_serial, &second_storage)?;
    let second_intent =
        SecretStorageReadIntent::from_inspection(second_storage, second_inspection)?;
    let second_document_storage = second_intent.storage.clone();
    let second = boundary.load_secret(primary_serial, second_intent, primary_pin.as_ref())?;
    let third_inspection = boundary.inspect_secret_storage_read(primary_serial, &third_storage)?;
    let third_intent = SecretStorageReadIntent::from_inspection(third_storage, third_inspection)?;
    let third_document_storage = third_intent.storage.clone();
    let third = boundary.load_secret(primary_serial, third_intent, primary_pin.as_ref())?;
    let document = BootstrapSecretDocument::from_storage_materials([
        (first_document_storage, first),
        (second_document_storage, second),
        (third_document_storage, third),
    ])?;
    for (storage, value) in document.storage_entries(spare_serial) {
        let inspection = boundary.inspect_secret_storage_write(spare_serial, &storage)?;
        let intent = SecretStorageWriteIntent::store(storage, inspection, value.len())?;
        boundary.store_secret(spare_serial, intent, value)?;
    }
    let spare_pin = if boundary.device_requires_pin(spare_serial)? {
        let pin = boundary.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    for storage in SecretStorageVerificationPlan::for_serial(spare_serial).into_targets() {
        let inspection = boundary.inspect_secret_storage_read(spare_serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let _secret = boundary.load_secret(spare_serial, intent, spare_pin.as_ref())?;
    }
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

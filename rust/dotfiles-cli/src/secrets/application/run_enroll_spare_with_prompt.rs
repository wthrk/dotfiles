use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
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
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let [
        bw_email_storage,
        bw_password_storage,
        bws_access_token_storage,
    ] = SecretStorageVerificationPlan::for_serial(primary_serial).into_targets();
    let bw_email_inspection =
        boundary.inspect_secret_storage_read(primary_serial, &bw_email_storage)?;
    let bw_email_intent =
        SecretStorageReadIntent::from_inspection(bw_email_storage, bw_email_inspection)?;
    let bw_email = boundary.load_secret(primary_serial, bw_email_intent, primary_pin.as_ref())?;
    let bw_password = boundary.inspect_secret_storage_read(primary_serial, &bw_password_storage)?;
    let bw_password_intent =
        SecretStorageReadIntent::from_inspection(bw_password_storage, bw_password)?;
    let bw_password =
        boundary.load_secret(primary_serial, bw_password_intent, primary_pin.as_ref())?;
    let bws_access_token_inspection =
        boundary.inspect_secret_storage_read(primary_serial, &bws_access_token_storage)?;
    let bws_access_token_intent = SecretStorageReadIntent::from_inspection(
        bws_access_token_storage,
        bws_access_token_inspection,
    )?;
    let bws_access_token = boundary.load_secret(
        primary_serial,
        bws_access_token_intent,
        primary_pin.as_ref(),
    )?;
    let document =
        BootstrapSecretDocument::from_secret_materials(&bw_email, &bw_password, &bws_access_token)?;
    for (storage, value) in document.storage_entries(spare_serial) {
        let inspection = boundary.inspect_secret_storage_write(spare_serial, &storage)?;
        let intent = SecretStorageWriteIntent::store(storage, inspection)?;
        boundary.store_secret(spare_serial, intent, value)?;
    }
    let spare_pin = if boundary.device_requires_pin(spare_serial)? {
        Some(boundary.read_pin()?)
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

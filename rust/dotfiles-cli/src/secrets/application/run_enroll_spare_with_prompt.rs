use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::{PivObjectId, SecretStorageSpec, StorageObjectIds},
        values::{EnrollSpareCommand, EnrollSummary},
    },
    ports::{self, SecretDevice},
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
        + ports::DeviceSelectionPort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let primary_serial = boundary.resolve_device_serial(command.primary_serial)?;
    let spare_serial = boundary.resolve_spare_device_serial(command.spare_serial)?;
    command.ensure_distinct_resolved_serials(primary_serial, spare_serial)?;
    let mut setup_device = boundary.open_device_by_serial(spare_serial)?;
    setup_device.check_key_generation_preconditions()?;
    setup_device.check_management_auth_preconditions()?;
    let key_exists = setup_device.key_exists()?;
    let manifest_bytes = setup_device.read_object(PivObjectId::MANIFEST)?;
    let mut occupied_object_ids = Vec::new();
    for object_id in StorageObjectIds::iter() {
        if setup_device.read_object(object_id)?.is_some() {
            occupied_object_ids.push(object_id);
        }
    }
    SecretManifest::ensure_setup_allowed(
        key_exists,
        manifest_bytes.as_deref(),
        &occupied_object_ids,
    )?;
    setup_device.generate_key()?;
    let mut manifest = SecretManifest::expected().encode()?;
    setup_device.write_object(PivObjectId::MANIFEST, &mut manifest)?;
    let primary_pin = if boundary.device_requires_pin(primary_serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut primary_device = boundary.open_device_by_serial(primary_serial)?;
    if primary_device.requires_pin_input() {
        let Some(pin) = primary_pin.as_ref() else {
            anyhow::bail!("PIN is required for this operation");
        };
        primary_device.verify_pin(pin)?;
    }
    SecretManifest::decode_initialized(
        primary_device
            .read_object(PivObjectId::MANIFEST)?
            .as_deref(),
    )?;
    let read_secret =
        |device: &mut B::Device, storage: SecretStorageSpec| -> Result<SecretMaterial> {
            let encoded = device
                .read_object(storage.object_id)?
                .ok_or_else(|| storage.missing_error())?;
            device
                .open_from_storage(storage.clone(), &encoded)
                .map_err(|error| storage.decode_error(error))
        };
    let [
        bw_email_storage,
        bw_password_storage,
        bws_access_token_storage,
    ] = SecretStorageSpec::all_for_serial(primary_serial);
    let bw_email = read_secret(&mut primary_device, bw_email_storage)?;
    let bw_password = read_secret(&mut primary_device, bw_password_storage)?;
    let bws_access_token = read_secret(&mut primary_device, bws_access_token_storage)?;
    let document =
        BootstrapSecretDocument::from_secret_materials(&bw_email, &bw_password, &bws_access_token)?;
    for (storage, value) in document.storage_entries(spare_serial) {
        let mut device = boundary.open_device_by_serial(spare_serial)?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        device.check_management_auth_preconditions()?;
        let mut encoded = device.seal_for_storage(storage.clone(), value)?;
        device.write_object(storage.object_id, &mut encoded)?;
    }
    let spare_pin = if boundary.device_requires_pin(spare_serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut verify_device = boundary.open_device_by_serial(spare_serial)?;
    if verify_device.requires_pin_input() {
        let Some(pin) = spare_pin.as_ref() else {
            anyhow::bail!("PIN is required for this operation");
        };
        verify_device.verify_pin(pin)?;
    }
    SecretManifest::decode_initialized(
        verify_device.read_object(PivObjectId::MANIFEST)?.as_deref(),
    )?;
    let read_spare_secret =
        |device: &mut B::Device, storage: SecretStorageSpec| -> Result<SecretMaterial> {
            let encoded = device
                .read_object(storage.object_id)?
                .ok_or_else(|| storage.missing_error())?;
            device
                .open_from_storage(storage.clone(), &encoded)
                .map_err(|error| storage.decode_error(error))
        };
    for storage in SecretStorageSpec::all_for_serial(spare_serial) {
        let _secret = read_spare_secret(&mut verify_device, storage)?;
    }
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

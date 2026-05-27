use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::PivObjectId,
        piv::SecretName,
        piv::StorageObjectIds,
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
    let read_secret = |device: &mut B::Device, name: SecretName| -> Result<SecretMaterial> {
        let encoded = device
            .read_object(name.object_id())?
            .ok_or_else(|| anyhow::anyhow!("{name} is not stored on this YubiKey"))?;
        device
            .open_from_storage(name.storage_spec(primary_serial), &encoded)
            .map_err(|error| anyhow::anyhow!("failed to decode {name}: {error}"))
    };
    let bw_email = read_secret(&mut primary_device, SecretName::BwEmail)?;
    let bw_password = read_secret(&mut primary_device, SecretName::BwPassword)?;
    let bws_access_token = read_secret(&mut primary_device, SecretName::BwsAccessToken)?;
    let document =
        BootstrapSecretDocument::from_secret_materials(&bw_email, &bw_password, &bws_access_token)?;
    for (name, value) in document.entries() {
        let mut device = boundary.open_device_by_serial(spare_serial)?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        device.check_management_auth_preconditions()?;
        let mut encoded = device.seal_for_storage(name.storage_spec(spare_serial), value)?;
        device.write_object(name.object_id(), &mut encoded)?;
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
    let read_spare_secret = |device: &mut B::Device, name: SecretName| -> Result<SecretMaterial> {
        let encoded = device
            .read_object(name.object_id())?
            .ok_or_else(|| anyhow::anyhow!("{name} is not stored on this YubiKey"))?;
        device
            .open_from_storage(name.storage_spec(spare_serial), &encoded)
            .map_err(|error| anyhow::anyhow!("failed to decode {name}: {error}"))
    };
    for name in SecretName::iter() {
        let _secret = read_spare_secret(&mut verify_device, name)?;
    }
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

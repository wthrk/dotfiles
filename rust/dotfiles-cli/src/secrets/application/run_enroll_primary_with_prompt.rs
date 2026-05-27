use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
        manifest::SecretManifest,
        piv::PivObjectId,
        piv::SecretName,
        piv::StorageObjectIds,
        values::{EnrollPrimaryCommand, EnrollSummary},
    },
    ports::{self, SecretDevice},
};

/// prompt 入力で primary YubiKey に bootstrap secret 一式を登録する。
///
/// 入力手段の詳細は `SecretInputPort` 側へ閉じ込め、use case は setup→store→verify の
/// 順序制御だけを担って application 層の責務境界を維持する。
pub(crate) fn run_enroll_primary_with_prompt<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::SecretInputPort
        + ports::ReportPort,
>(
    command: EnrollPrimaryCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let mut setup_device = boundary.open_device_by_serial(serial)?;
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
    let bw_email = boundary.read_visible_secret()?;
    let bw_password = boundary.read_hidden_secret(SecretName::BwPassword)?;
    let bws_access_token = boundary.read_hidden_secret(SecretName::BwsAccessToken)?;
    let document =
        BootstrapSecretDocument::from_secret_materials(&bw_email, &bw_password, &bws_access_token)?;
    for (name, value) in document.entries() {
        let mut device = boundary.open_device_by_serial(serial)?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        device.check_management_auth_preconditions()?;
        let mut encoded = device.seal_for_storage(name.storage_spec(serial), value)?;
        device.write_object(name.object_id(), &mut encoded)?;
    }
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut verify_device = boundary.open_device_by_serial(serial)?;
    if verify_device.requires_pin_input() {
        let Some(pin) = pin.as_ref() else {
            anyhow::bail!("PIN is required for this operation");
        };
        verify_device.verify_pin(pin)?;
    }
    SecretManifest::decode_initialized(
        verify_device.read_object(PivObjectId::MANIFEST)?.as_deref(),
    )?;
    for name in SecretName::iter() {
        let encoded = verify_device
            .read_object(name.object_id())?
            .ok_or_else(|| anyhow::anyhow!("{name} is not stored on this YubiKey"))?;
        let _secret = verify_device
            .open_from_storage(name.storage_spec(serial), &encoded)
            .map_err(|error| anyhow::anyhow!("failed to decode {name}: {error}"))?;
    }
    boundary.write_enroll_report(&EnrollSummary::primary_completed(serial))
}

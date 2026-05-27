use crate::Result;
use crate::secrets::{
    domain::{
        blob::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob},
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::PivObjectId,
        piv::SecretName,
        piv::StorageObjectIds,
        values::{EnrollSpareCommand, EnrollSummary},
    },
    ports::{self, SecretDevice},
};

/// stdin JSON document で spare YubiKey に bootstrap secret 一式を登録する。
///
/// primary と spare の衝突停止条件を先に評価し、device 選択・入力実装は port に委譲して
/// use case の順序責務だけを保持する。
pub(crate) fn run_enroll_spare_with_stdin_json<
    B: ports::SpareDeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::BootstrapSecretDocumentInputPort
        + ports::RandomBytesPort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let spare_serial =
        boundary.resolve_spare_device_serial(command.primary_serial, command.spare_serial)?;
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
    let document = boundary.read_bootstrap_secret_document_noninteractive()?;
    for (name, value) in [
        (SecretName::BwEmail, &document.bw_email),
        (SecretName::BwPassword, &document.bw_password),
        (SecretName::BwsAccessToken, &document.bws_access_token),
    ] {
        let mut device = boundary.open_device_by_serial(spare_serial)?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        device.check_management_auth_preconditions()?;
        let mut content_key = SecretMaterial::new(CONTENT_KEY_LEN)?;
        content_key.with_secret_mut(|bytes| boundary.fill_random_bytes(bytes))?;
        let mut nonce = [0u8; NONCE_LEN];
        boundary.fill_random_bytes(&mut nonce)?;
        let wrapped_key = device.wrap_key(&content_key)?;
        let blob = SecretBlob::encrypt_secret_for_storage(
            name,
            device.serial(),
            nonce,
            wrapped_key,
            value,
            &content_key,
        )?;
        let mut encoded = blob.encode()?;
        device.write_object(name.object_id(), &mut encoded)?;
    }
    let pin = if boundary.device_requires_pin(spare_serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut verify_device = boundary.open_device_by_serial(spare_serial)?;
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
        let wrapped_key = SecretBlob::decode_for_name(&encoded, name)?.wrapped_key;
        let content_key = verify_device.unwrap_key(&wrapped_key)?;
        let _secret = SecretBlob::decode_decrypt_and_validate(
            &encoded,
            name,
            verify_device.serial(),
            &content_key,
        )?;
    }
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

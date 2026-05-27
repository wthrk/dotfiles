use crate::Result;
use crate::secrets::{
    domain::{
        blob::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob},
        manifest::BootstrapSecretDocument,
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::PivObjectId,
        piv::SecretName,
        piv::StorageObjectIds,
        values::{EnrollSpareCommand, EnrollSummary},
    },
    ports::{self, SecretDevice},
    support::aead::{aes_256_gcm_from_key, decrypt_detached, encrypt_detached},
};

/// primary YubiKey から読み出した secret を prompt 運用の spare YubiKey へ複製する。
///
/// primary/spare 解決順序を固定して同一 serial への誤登録を防ぎ、secret 転送手段の詳細は
/// `SecretLoadPort` / `SecretStorePort` 境界へ閉じ込める。
pub(crate) fn run_enroll_spare_with_prompt<
    B: ports::DeviceSerialPort
        + ports::SpareDeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::RandomBytesPort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let primary_serial = boundary.resolve_device_serial(command.primary_serial)?;
    let spare_serial =
        boundary.resolve_spare_device_serial(Some(primary_serial), command.spare_serial)?;
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
        let blob = SecretBlob::decode(&encoded)
            .map_err(|error| anyhow::anyhow!("failed to decode {name}: {error}"))?;
        if blob.name != name {
            anyhow::bail!("YubiKey secret blob name does not match requested {}", name);
        }
        let SecretBlob {
            name: blob_name,
            nonce,
            wrapped_key,
            ciphertext,
            tag,
        } = blob;
        let content_key = device.unwrap_key(&wrapped_key)?;
        if content_key.len() != CONTENT_KEY_LEN {
            anyhow::bail!("unwrapped YubiKey content key has invalid length");
        }
        let cipher = content_key.with_bytes(aes_256_gcm_from_key)?;
        let mut secret = ciphertext;
        secret
            .with_secret_mut(|secret_bytes| {
                decrypt_detached(
                    &cipher,
                    &nonce,
                    &blob_name.additional_data(device.serial()),
                    secret_bytes,
                    &tag,
                )
            })
            .map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob_name))?;
        Ok(secret)
    };
    let bw_email = read_secret(&mut primary_device, SecretName::BwEmail)?;
    let bw_password = read_secret(&mut primary_device, SecretName::BwPassword)?;
    let bws_access_token = read_secret(&mut primary_device, SecretName::BwsAccessToken)?;
    let document = BootstrapSecretDocument::from_interactive_secrets(
        bw_email.as_ref(),
        bw_password.as_ref(),
        bws_access_token.as_ref(),
    )?;
    for (name, value) in [
        (SecretName::BwEmail, &document.bw_email),
        (SecretName::BwPassword, &document.bw_password),
        (SecretName::BwsAccessToken, &document.bws_access_token),
    ] {
        let mut device = boundary.open_device_by_serial(spare_serial)?;
        value.with_bytes(|bytes| name.ensure_value_non_empty(bytes))?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        device.check_management_auth_preconditions()?;
        let mut content_key = SecretMaterial::new(CONTENT_KEY_LEN)?;
        content_key.with_secret_mut(|bytes| boundary.fill_random_bytes(bytes))?;
        let mut nonce = [0u8; NONCE_LEN];
        boundary.fill_random_bytes(&mut nonce)?;
        let cipher = content_key.with_bytes(aes_256_gcm_from_key)?;
        let mut ciphertext = value.with_bytes(SecretMaterial::copy_from_slice)?;
        let tag = ciphertext.with_secret_mut(|ciphertext_bytes| {
            encrypt_detached(
                &cipher,
                &nonce,
                &name.additional_data(device.serial()),
                ciphertext_bytes,
            )
        })?;
        let wrapped_key = device.wrap_key(&content_key)?;
        let blob = SecretBlob {
            name,
            nonce,
            wrapped_key,
            ciphertext,
            tag,
        };
        let mut encoded = blob.encode()?;
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
    for name in SecretName::iter() {
        let secret = read_secret(&mut verify_device, name)?;
        secret.with_bytes(|bytes| name.ensure_value_non_empty(bytes))?;
    }
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

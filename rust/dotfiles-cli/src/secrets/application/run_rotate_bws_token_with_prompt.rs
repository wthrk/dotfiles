use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{
        blob::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob},
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::PivObjectId,
        piv::SecretName,
        values::{RotateBwsTokenCommand, VerifySummary},
    },
    ports::{self, SecretDevice},
};

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// prompt 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// serial 未指定時は非対話運用の誤書き込みを防ぐため停止し、保存失敗と検証失敗の責務は
/// `SecretStorePort` / `StorageVerifyPort` の境界へ分離する。
pub(crate) fn run_rotate_bws_token_with_prompt<
    B: ports::SecretInputPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::RandomBytesPort
        + ports::ReportPort,
>(
    command: RotateBwsTokenCommand,
    boundary: &mut B,
) -> Result<()> {
    let Some(serial) = command.serial else {
        bail!(NONINTERACTIVE_SERIAL_ERROR);
    };
    let token = boundary.read_hidden_secret(SecretName::BwsAccessToken)?;
    let mut device = boundary.open_device_by_serial(serial)?;
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    device.check_management_auth_preconditions()?;
    let mut content_key = SecretMaterial::new(CONTENT_KEY_LEN)?;
    content_key.with_secret_mut(|value| boundary.fill_random_bytes(value))?;
    let mut nonce = [0u8; NONCE_LEN];
    boundary.fill_random_bytes(&mut nonce)?;
    let wrapped_key = device.wrap_key(&content_key)?;
    let blob = SecretBlob::encrypt_secret_for_storage(
        SecretName::BwsAccessToken,
        device.serial(),
        nonce,
        wrapped_key,
        &token,
        &content_key,
    )?;
    let mut encoded = blob.encode()?;
    device.write_object(SecretName::BwsAccessToken.object_id(), &mut encoded)?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut verify_device = boundary.open_device_by_serial(serial)?;
    if verify_device.requires_pin_input() {
        let Some(pin) = pin.as_ref() else {
            bail!("PIN is required for this operation");
        };
        verify_device.verify_pin(pin)?;
    }
    let verify_result = (|| -> Result<()> {
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
        Ok(())
    })();
    match verify_result {
        Ok(()) => boundary.write_verify_report(&VerifySummary::local_storage_verified(serial)),
        Err(err) => boundary
            .write_verify_report(&VerifySummary::local_storage_failed(serial))
            .and(Err(err)),
    }
}

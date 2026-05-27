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
    support::aead::{aes_256_gcm_from_key, decrypt_detached, encrypt_detached},
};

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// stdin 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// token 読み取り方式は port 境界で差し替え、use case 側では serial 必須条件と
/// 保存後検証の順序のみを固定して責務混在を避ける。
pub(crate) fn run_rotate_bws_token_with_stdin<
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
    let token = boundary.read_stdin_secret()?;
    let mut device = boundary.open_device_by_serial(serial)?;
    token.with_bytes(|bytes| SecretName::BwsAccessToken.ensure_value_non_empty(bytes))?;
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    device.check_management_auth_preconditions()?;
    let mut content_key = SecretMaterial::new(CONTENT_KEY_LEN)?;
    content_key.with_secret_mut(|value| boundary.fill_random_bytes(value))?;
    let mut nonce = [0u8; NONCE_LEN];
    boundary.fill_random_bytes(&mut nonce)?;
    let cipher = content_key.with_bytes(aes_256_gcm_from_key)?;
    let mut ciphertext = token.with_bytes(SecretMaterial::copy_from_slice)?;
    let tag = ciphertext.with_secret_mut(|ciphertext_bytes| {
        encrypt_detached(
            &cipher,
            &nonce,
            &SecretName::BwsAccessToken.additional_data(device.serial()),
            ciphertext_bytes,
        )
    })?;
    let wrapped_key = device.wrap_key(&content_key)?;
    let blob = SecretBlob {
        name: SecretName::BwsAccessToken,
        nonce,
        wrapped_key,
        ciphertext,
        tag,
    };
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
            let blob = SecretBlob::decode(&encoded)
                .map_err(|error| anyhow::anyhow!("failed to decode {name}: {error}"))?;
            if blob.name != name {
                bail!("YubiKey secret blob name does not match requested {}", name);
            }
            let SecretBlob {
                name: blob_name,
                nonce,
                wrapped_key,
                ciphertext,
                tag,
            } = blob;
            let content_key = verify_device.unwrap_key(&wrapped_key)?;
            if content_key.len() != CONTENT_KEY_LEN {
                bail!("unwrapped YubiKey content key has invalid length");
            }
            let cipher = content_key.with_bytes(aes_256_gcm_from_key)?;
            let mut secret = ciphertext;
            secret
                .with_secret_mut(|secret_bytes| {
                    decrypt_detached(
                        &cipher,
                        &nonce,
                        &blob_name.additional_data(verify_device.serial()),
                        secret_bytes,
                        &tag,
                    )
                })
                .map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob_name))?;
            secret.with_bytes(|bytes| name.ensure_value_non_empty(bytes))?;
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

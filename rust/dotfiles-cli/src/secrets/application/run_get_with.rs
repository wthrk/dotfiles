use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{
        blob::{CONTENT_KEY_LEN, SecretBlob},
        manifest::SecretManifest,
        piv::PivObjectId,
        values::GetCommand,
    },
    ports::{self, SecretDevice},
    support::aead::{aes_256_gcm_from_key, decrypt_detached},
};

/// 指定された secret を YubiKey storage から読み出し、出力 port へ受け渡す。
///
/// 読み出し経路の secret 値を application 層で加工せず、復号と出力方針は adapter 側の責務境界へ固定する。
pub(crate) fn run_get_with<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::SecretOutputPort,
>(
    command: GetCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut device = boundary.open_device_by_serial(serial)?;
    if device.requires_pin_input() {
        let Some(pin) = pin.as_ref() else {
            bail!("PIN is required for this operation");
        };
        device.verify_pin(pin)?;
    }
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    let encoded = device
        .read_object(command.name.object_id())?
        .ok_or_else(|| anyhow::anyhow!("{} is not stored on this YubiKey", command.name))?;
    let blob = SecretBlob::decode(&encoded)
        .map_err(|error| anyhow::anyhow!("failed to decode {}: {error}", command.name))?;
    if blob.name != command.name {
        bail!(
            "YubiKey secret blob name does not match requested {}",
            command.name
        );
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
        bail!("unwrapped YubiKey content key has invalid length");
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
    boundary.write_secret(&secret)
}

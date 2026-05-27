use crate::Result;
use crate::secrets::{
    domain::{
        blob::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob},
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::PivObjectId,
        values::PutCommand,
    },
    ports::{self, SecretDevice},
    support::aead::{aes_256_gcm_from_key, encrypt_detached},
};

/// 対話入力で取得した secret を対象 serial の YubiKey storage へ保存する。
///
/// 入力モードの可視/不可視判定は `SecretName` の domain 規則で決め、端末 I/O 実装詳細は adapter へ委譲する。
pub(crate) fn run_put_with_prompt<
    B: ports::DeviceSerialPort
        + ports::SecretInputPort
        + ports::DeviceSelectionPort
        + ports::RandomBytesPort,
>(
    command: PutCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let secret = if command.name.uses_visible_input() {
        boundary.read_visible_secret()?
    } else {
        boundary.read_hidden_secret(command.name)?
    };
    let mut device = boundary.open_device_by_serial(serial)?;
    secret.with_bytes(|bytes| command.name.ensure_value_non_empty(bytes))?;
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    device.check_management_auth_preconditions()?;
    if device.read_object(command.name.object_id())?.is_some() && !command.force {
        anyhow::bail!(
            "{} already exists; pass --force to replace it",
            command.name
        );
    }
    let mut content_key = SecretMaterial::new(CONTENT_KEY_LEN)?;
    content_key.with_secret_mut(|value| boundary.fill_random_bytes(value))?;
    let mut nonce = [0u8; NONCE_LEN];
    boundary.fill_random_bytes(&mut nonce)?;
    let cipher = content_key.with_bytes(aes_256_gcm_from_key)?;
    let mut ciphertext = secret.with_bytes(SecretMaterial::copy_from_slice)?;
    let tag = ciphertext.with_secret_mut(|ciphertext_bytes| {
        encrypt_detached(
            &cipher,
            &nonce,
            &command.name.additional_data(device.serial()),
            ciphertext_bytes,
        )
    })?;
    let wrapped_key = device.wrap_key(&content_key)?;
    let blob = SecretBlob {
        name: command.name,
        nonce,
        wrapped_key,
        ciphertext,
        tag,
    };
    let mut encoded = blob.encode()?;
    device.write_object(command.name.object_id(), &mut encoded)
}

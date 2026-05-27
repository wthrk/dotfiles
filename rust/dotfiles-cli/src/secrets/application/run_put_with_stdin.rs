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

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// 非対話 stdin から受け取った secret を対象 serial の YubiKey storage へ保存する。
///
/// use case は入力取得と保存順序のみを担い、stdin 条件やサイズ制約は adapter 実装側へ閉じ込める。
pub(crate) fn run_put_with_stdin<
    B: ports::SecretInputPort + ports::DeviceSelectionPort + ports::RandomBytesPort,
>(
    command: PutCommand,
    boundary: &mut B,
) -> Result<()> {
    let Some(serial) = command.serial else {
        anyhow::bail!(NONINTERACTIVE_SERIAL_ERROR);
    };
    let secret = boundary.read_stdin_secret()?;
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

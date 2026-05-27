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
    let wrapped_key = device.wrap_key(&content_key)?;
    let blob = SecretBlob::encrypt_secret_for_storage(
        command.name,
        device.serial(),
        nonce,
        wrapped_key,
        &secret,
        &content_key,
    )?;
    let mut encoded = blob.encode()?;
    device.write_object(command.name.object_id(), &mut encoded)
}

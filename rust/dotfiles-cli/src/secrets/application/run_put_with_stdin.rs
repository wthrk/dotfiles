use crate::Result;
use crate::secrets::{
    domain::{manifest::SecretManifest, piv::PivObjectId, values::PutCommand},
    ports::{self, SecretDevice},
};

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// 非対話 stdin から受け取った secret を対象 serial の YubiKey storage へ保存する。
///
/// use case は入力取得と保存順序のみを担い、stdin 条件やサイズ制約は adapter 実装側へ閉じ込める。
pub(crate) fn run_put_with_stdin<B: ports::SecretInputPort + ports::DeviceSelectionPort>(
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
    let storage = command.storage_spec(serial);
    command.name.ensure_write_allowed(
        device.read_object(storage.object_id)?.is_some(),
        command.force,
    )?;
    let mut encoded = device.seal_for_storage(storage.clone(), &secret)?;
    device.write_object(storage.object_id, &mut encoded)
}

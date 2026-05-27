use crate::Result;
use crate::secrets::{
    domain::{manifest::SecretManifest, piv::PivObjectId, values::PutCommand},
    ports::{self, SecretDevice},
};

/// 対話入力で取得した secret を対象 serial の YubiKey storage へ保存する。
///
/// 入力モードの可視/不可視判定は `SecretName` の domain 規則で決め、端末 I/O 実装詳細は adapter へ委譲する。
pub(crate) fn run_put_with_prompt<
    B: ports::DeviceSerialPort + ports::SecretInputPort + ports::DeviceSelectionPort,
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
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    device.check_management_auth_preconditions()?;
    if device.read_object(command.name.object_id())?.is_some() && !command.force {
        anyhow::bail!(
            "{} already exists; pass --force to replace it",
            command.name
        );
    }
    let mut encoded = device.seal_for_storage(command.name, &secret)?;
    device.write_object(command.name.object_id(), &mut encoded)
}

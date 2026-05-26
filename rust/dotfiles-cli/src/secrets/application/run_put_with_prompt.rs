use crate::Result;
use crate::secrets::{
    domain::values::PutCommand,
    ports::{self},
};

/// 対話入力で取得した secret を対象 serial の YubiKey storage へ保存する。
///
/// 入力モードの可視/不可視判定は `SecretName` の domain 規則で決め、端末 I/O 実装詳細は adapter へ委譲する。
pub(crate) fn run_put_with_prompt<
    B: ports::DeviceSerialPort + ports::SecretInputPort + ports::SecretStorePort,
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
    boundary.store_secret(serial, command.name, command.force, &secret)
}

use crate::Result;
use crate::secrets::{
    domain::{PutCommand, SecretName},
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
    let secret = read_secret_for_name(boundary, command.name)?;
    boundary.store_secret(serial, command.name, command.force, secret.as_ref())
}

fn read_secret_for_name<B: ports::SecretInputPort>(
    boundary: &B,
    name: SecretName,
) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    let label = format!("{name}: ");
    if name.uses_visible_input() {
        boundary.read_visible_secret(&label)
    } else {
        boundary.read_hidden_secret(&label)
    }
}

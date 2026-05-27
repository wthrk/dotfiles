use crate::Result;
use crate::secrets::{
    domain::{storage::SecretStorageWriteIntent, values::PutCommand},
    ports::{self, SecretStoragePort},
};

/// 対話入力で取得した secret を対象 serial の YubiKey storage へ保存する。
///
/// 入力モードの可視/不可視判定は `SecretName` の domain 規則で決め、端末 I/O 実装詳細は adapter へ委譲する。
pub(crate) fn run_put_with_prompt<
    B: ports::DeviceSerialPort + ports::SecretInputPort + SecretStoragePort,
>(
    command: PutCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let secret = command.name.read_interactive_secret_with(
        || boundary.read_bw_email_secret(),
        || boundary.read_bw_password_secret(),
        || boundary.read_bws_access_token_secret(),
    )?;
    let storage = command.storage_spec(serial);
    let inspection = boundary.inspect_secret_storage_write(serial, &storage)?;
    let intent = SecretStorageWriteIntent::put(storage, inspection, command.force)?;
    boundary.store_secret(serial, intent, &secret)
}

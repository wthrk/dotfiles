use crate::Result;
use crate::secrets::{
    domain::{storage::SecretStorageWriteIntent, values::PutCommand},
    ports::{self, SecretStoragePort},
};

/// 非対話 stdin から受け取った secret を対象 serial の YubiKey storage へ保存する。
///
/// use case は入力取得と保存順序のみを担い、stdin 条件やサイズ制約は adapter 実装側へ閉じ込める。
pub(crate) fn run_put_with_stdin<B: ports::SecretInputPort + SecretStoragePort>(
    command: PutCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = command.required_serial()?;
    let secret = boundary.read_streamed_secret()?;
    let storage = command.storage_spec(serial);
    let inspection = boundary.inspect_secret_storage_write(serial, &storage)?;
    let intent = SecretStorageWriteIntent::put(storage, inspection, command.force, secret.len())?;
    boundary.store_secret(serial, intent, &secret)
}

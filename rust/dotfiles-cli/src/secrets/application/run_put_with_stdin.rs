use crate::Result;
use crate::secrets::{
    domain::values::PutCommand,
    ports::{self},
};

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// 非対話 stdin から受け取った secret を対象 serial の YubiKey storage へ保存する。
///
/// use case は入力取得と保存順序のみを担い、stdin 条件やサイズ制約は adapter 実装側へ閉じ込める。
pub(crate) fn run_put_with_stdin<B: ports::SecretInputPort + ports::SecretStorePort>(
    command: PutCommand,
    boundary: &mut B,
) -> Result<()> {
    let Some(serial) = command.serial else {
        anyhow::bail!(NONINTERACTIVE_SERIAL_ERROR);
    };
    let secret = boundary.read_stdin_secret()?;
    boundary.store_secret(serial, command.name, command.force, &secret)
}

//! stdout への secret 書き込み adapter。
//!
//! stdout が TTY でないことを確認してから復号済み bytes を書き込む。

use anyhow::bail;

use super::terminal;
use crate::Result;

const SECRET_STDOUT_TERMINAL_ERROR: &str =
    "refusing to write secret to terminal; redirect stdout to a file or pipe";

/// stdout が TTY の場合は、復号結果を書き込む前に利用者向け error で停止する。
fn ensure_secret_stdout_not_terminal() -> Result<()> {
    if terminal::stdout_is_terminal() {
        bail!(SECRET_STDOUT_TERMINAL_ERROR);
    }
    Ok(())
}

/// stdout の TTY 拒否を確認してから、復号済み bytes を stdout へ書き込む。
pub(crate) fn write_secret_to_stdout(bytes: &[u8]) -> Result<()> {
    ensure_secret_stdout_not_terminal()?;
    terminal::write_all_stdout(bytes)
}

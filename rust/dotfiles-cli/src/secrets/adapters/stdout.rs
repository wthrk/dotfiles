//! stdout への secret 書き込み adapter。
//!
//! stdout が TTY でないことを確認してから復号済み bytes を書き込む。

use anyhow::bail;

use super::terminal;
use crate::Result;

pub(crate) const SECRET_STDOUT_TERMINAL_ERROR: &str =
    "refusing to write secret to terminal; redirect stdout to a file or pipe";

/// stdout が TTY の場合は、復号結果を書き込む前に利用者向け error で停止する。
pub(crate) fn ensure_secret_stdout_not_terminal() -> Result<()> {
    if terminal::stdout_is_terminal() {
        reject_secret_stdout_terminal()?;
    }
    Ok(())
}

/// stdout の TTY 拒否を確認してから、復号済み bytes を stdout へ書き込む。
pub(crate) fn write_secret_to_stdout(bytes: &[u8]) -> Result<()> {
    ensure_secret_stdout_not_terminal()?;
    terminal::write_all_stdout(bytes)
}

/// stdout が TTY の場合に返す利用者向け error を、実プロセス境界と test 境界で共有する。
pub(crate) fn reject_secret_stdout_terminal() -> Result<()> {
    bail!(SECRET_STDOUT_TERMINAL_ERROR);
}

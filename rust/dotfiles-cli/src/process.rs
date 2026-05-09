//! `dotfiles` が呼ぶ外部コマンドの実行と表示を揃える。
//!
//! `dry_run` では実行せず、実行予定のコマンドだけを shell 風に出力する。実行時は終了状態を見て、
//! 失敗したプログラム名をエラーに含める。

use std::ffi::OsString;
use std::process::Command;

use crate::Result;
use anyhow::bail;
use dotfiles_core::command;

/// `dry_run` なら表示のみ、通常時は同じ表示形式で実行して非 0 終了を失敗にする。
pub(crate) fn run<I>(program: impl Into<OsString>, args: I, dry_run: bool) -> Result<()>
where
    I: IntoIterator<Item = OsString>,
{
    let program = program.into();
    let args = args.into_iter().collect::<Vec<_>>();
    println!("$ {}", command::display(&program, &args));
    if dry_run {
        return Ok(());
    }

    let status = Command::new(&program).args(&args).status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {}: {status}", program.to_string_lossy())
    }
}

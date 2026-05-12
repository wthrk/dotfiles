//! `cargo xtask` として実行される保守用バイナリ。
//!
//! 利用者向けの `dotfiles` とは分離し、検証やローカル適用のコマンドをここに固定する。
//! 実処理はサブモジュールへ置き、main は終了コードへの変換だけを行う。

use std::process::ExitCode;

mod apply;
mod check;
mod cli;

type Result<T> = dotfiles_core::Result<T>;

fn main() -> ExitCode {
    match cli::dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

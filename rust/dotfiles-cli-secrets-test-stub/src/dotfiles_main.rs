//! CLI 統合テスト向けの `dotfiles` production binary エントリーポイント。
//!
//! テストが `CARGO_BIN_EXE_dotfiles` で production binary を参照できるよう、
//! `dotfiles_cli::dispatch` を呼ぶ thin wrapper を提供する。
//! この binary は production の `dotfiles` と同一の挙動をし、test double は含まない。

use std::process::ExitCode;

fn main() -> ExitCode {
    match dotfiles_cli::dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

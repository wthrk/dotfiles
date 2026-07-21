//! `secrets_cli` integration test 専用の compile-time internal-stub binary。
//!
//! 通常 CLI と同じ dispatch entrypoint を持つが、Cargo target 名だけを分離する。
//! これにより `secrets-internal-test-stub` feature を要求する artifact は通常の
//! featureless `dotfiles` artifact と同じ出力 path を共有しない。

use std::process::ExitCode;

fn main() -> ExitCode {
    match dotfiles_cli::dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(dotfiles_cli::exit_code_for_error(&err))
        }
    }
}

//! ユーザー向け `dotfiles` コマンドのプロセス境界。
//!
//! ここでは clap の解析結果を実処理へ渡し、エラーを標準エラーと終了コードへ変換する。
//! 生成、環境検出、外部コマンド実行はそれぞれのモジュールに分ける。

use std::process::ExitCode;

mod cli;
mod environment;
mod init;
mod local_flake;
mod process;
mod switch;
mod update;

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

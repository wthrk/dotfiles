//! ユーザー向け `dotfiles` コマンドのプロセス境界。
//!
//! ここでは clap の解析結果を実処理へ渡し、エラーを標準エラーと終了コードへ変換する。
//! 実装ロジックは library crate（`dotfiles_cli`）側に置き、binary は dispatch だけを担う。

use anyhow::Error;
use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match dotfiles_cli::dispatch().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            render_error_chain(&err);
            ExitCode::FAILURE
        }
    }
}

fn render_error_chain(error: &Error) {
    eprintln!("{error}");
    let mut chain = error.chain().skip(1).peekable();
    if chain.peek().is_none() {
        return;
    }
    eprintln!("caused by:");
    for (index, cause) in chain.enumerate() {
        eprintln!("  {}: {}", index + 1, cause);
    }
}

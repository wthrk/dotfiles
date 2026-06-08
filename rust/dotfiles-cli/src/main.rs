//! ユーザー向け `dotfiles` コマンドのプロセス境界。
//!
//! ここでは clap の解析結果を実処理へ渡し、エラーを標準エラーと終了コードへ変換する。
//! 実装ロジックは library crate（`dotfiles_cli`）側に置き、binary は dispatch だけを担う。

use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    // dispatch が exit code を確定する（`update` の lock 競合 skip は専用 code。詳細は cli::dispatch）。
    // エラー時の stderr 出力も dispatch 内で済ませる。
    dotfiles_cli::dispatch().await
}

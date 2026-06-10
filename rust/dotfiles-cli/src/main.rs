//! ユーザー向け `dotfiles` コマンドのプロセス境界。
//!
//! ここでは clap の解析結果を実処理へ渡し、エラーを標準エラーと終了コードへ変換する。
//! 実装ロジックは library crate（`dotfiles_cli`）側に置き、binary は dispatch だけを担う。

use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    // dispatch が各コマンドの `Result` を `ExitCode` へ変換し、エラー時は stderr へ出力してから非 0 を返す。
    dotfiles_cli::dispatch().await
}

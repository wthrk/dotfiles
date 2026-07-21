//! ユーザー向け `dotfiles` コマンドのプロセス境界。
//!
//! ここでは clap の解析結果を実処理へ渡し、エラーを標準エラーと終了コードへ変換する。
//! 実装ロジックは library crate（`dotfiles_cli`）側に置き、binary は dispatch だけを担う。

// `secrets-internal-test-stub` は integration test 専用 target だけの compile-time
// backend である。stub build は `--no-default-features` で `production-cli` を外すため
// normal target は artifact graph に入らない。両 feature を明示して通常 target へ stub /
// observation channel を link する構成は compile 自体を拒否する。
#[cfg(all(feature = "production-cli", feature = "secrets-internal-test-stub"))]
compile_error!(
    "`secrets-internal-test-stub` is only permitted for the dotfiles-secrets-internal-test-stub test binary"
);

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

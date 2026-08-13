//! ゲスト内で実行する統合シナリオのプロセス境界。
//!
//! Tart VM でも GitHub Actions の runner でも、ゲスト自身がこの実行器をビルドして起動する。
//! 選択されたシナリオは同じ `ScenarioRunner` へ渡る。

use std::process::ExitCode;

use clap::Parser;
use scenario::RuntimeScenario;

mod assertions;
mod command;
mod runtime_env;
mod scenario;
mod users;

type Result<T> = dotfiles_core::Result<T>;

#[derive(Parser)]
#[command(name = "dotfiles-integration-test-guest")]
/// clap が受け取るシナリオ。未指定時は full を実行する。
struct Args {
    #[arg(value_enum)]
    scenario: Option<RuntimeScenario>,
    /// 検証対象 commit。bootstrap と生成 flake が `github:wthrk/dotfiles/<sha>` として参照する。
    #[arg(long, env = "DOTFILES_TEST_SOURCE_HASH")]
    source_hash: String,
}

/// シナリオ中のコマンド失敗を標準エラーへ出し、ホスト側 SSH に非 0 終了を返す。
fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// ゲスト環境を検出してから、選択された順序付きシナリオを実行する。
fn run(args: Args) -> Result<()> {
    scenario::ScenarioRunner::new(&args.source_hash)?
        .run_scenario(args.scenario.unwrap_or(RuntimeScenario::Full))
}

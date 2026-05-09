//! ゲスト内で実行する統合シナリオのプロセス境界。
//!
//! ホスト側 runner からコピーされる場合も、GitHub Actions 内で直接起動される場合も、
//! ここで選択されたシナリオを同じ `ScenarioRunner` に渡す。

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
    scenario::ScenarioRunner::new()?.run_scenario(args.scenario.unwrap_or(RuntimeScenario::Full))
}

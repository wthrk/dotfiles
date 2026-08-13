//! Tart ゲストを使う runtime 統合検証の起動。

use clap::ValueEnum;
use xshell::{Shell, cmd};

use crate::Result;

#[derive(Clone, Copy, ValueEnum)]
/// 統合テスト実行器へ渡すシナリオ。現状は初期設定から switch までの full のみ。
pub(crate) enum RuntimeScenario {
    Full,
}

/// VM の準備と guest 実行は integration クレート側へ任せる。
pub(crate) fn run(scenario: RuntimeScenario, source_hash: &str) -> Result<()> {
    let shell = Shell::new()?;
    match scenario {
        RuntimeScenario::Full => {
            cmd!(
                shell,
                "cargo run --package dotfiles-integration-tests -- --source-hash {source_hash}"
            )
            .run()?;
        }
    }
    Ok(())
}

//! リポジトリ保守コマンドの clap 定義。

use clap::{Parser, Subcommand, ValueEnum};

use crate::{Result, apply, check, ci};

#[derive(Parser)]
#[command(name = "xtask")]
/// 公開 CLI ではなく、開発者が `cargo xtask ...` として呼ぶ最上位コマンド。
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
/// 検証だけを行う `check`、検証後にローカル適用する `apply`、CI 機械判定の `ci`。
enum Command {
    Apply {
        #[arg(value_enum, default_value_t = ApplyTarget::All)]
        target: ApplyTarget,
    },
    Check {
        #[command(subcommand)]
        target: Option<CheckTarget>,
    },
    Ci(ci::CiOptions),
}

#[derive(Clone, Copy, ValueEnum)]
/// `apply` が Home Manager だけを動かすか、Darwin まで含めるかの選択肢。
pub(crate) enum ApplyTarget {
    All,
    HomeManager,
}

#[derive(Clone, Copy, ValueEnum)]
/// runtime VM を使う統合検証のシナリオ選択肢。現状は一続きの full のみ。
pub(crate) enum RuntimeScenario {
    Full,
}

#[derive(Subcommand)]
/// `dotfiles-checks` へ渡す検証対象。static と test は責務を分離し、runtime は VM が必要なため明示選択にする。
pub(crate) enum CheckTarget {
    Static,
    Test,
    Zsh,
    Runtime {
        #[arg(value_enum)]
        scenario: Option<RuntimeScenario>,
        #[arg(long, env = "DOTFILES_TEST_SOURCE_HASH")]
        source_hash: Option<String>,
    },
    All,
}

/// clap の結果を `check` / `apply` の実装へ渡し、ここでは外部コマンドを直接起動しない。
pub fn dispatch() -> Result<()> {
    match Cli::parse().command {
        Command::Apply { target } => apply::run(target),
        Command::Check { target } => check::run(target),
        Command::Ci(options) => ci::run(options),
    }
}

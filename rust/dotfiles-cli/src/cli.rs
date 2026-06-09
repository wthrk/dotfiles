//! 利用者に公開する `dotfiles` コマンドの clap 定義。
//!
//! 環境変数で補える値は clap の `env` に寄せる。ここに置くサブコマンドは利用者が直接
//! 実行する操作に限定し、リポジトリ保守用の操作は `xtask` 側へ分ける。

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::update::UpdateOutcome;

#[derive(Parser)]
#[command(name = "dotfiles")]
/// 利用者がローカル flake を作成、更新、適用するための最上位 CLI。
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
/// ローカル flake 操作、秘密情報復旧、設定適用を利用者向け command として公開する。
enum Command {
    Ci(crate::ci::CiOptions),
    Init(crate::init::InitOptions),
    Secrets(dotfiles_secrets::SecretsOptions),
    Gpg(dotfiles_secrets::GpgOptions),
    Switch(crate::switch::SwitchOptions),
    Update(crate::update::UpdateOptions),
    UpdateHistory(crate::update_history::UpdateHistoryOptions),
}

/// clap が確定したオプションだけを各処理へ渡し、ここでは実行ロジックを持たない。
///
/// 単純版の `update` は排他/lock を持たないため、`update` も他 command と同じく `Ok` → 0 / `Err` → 1 で扱う
/// （lock 競合 skip 専用 exit code は撤去した。`UpdateOutcome::Completed` は成功＝0）。
pub(crate) async fn dispatch() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Ci(options) => crate::ci::run(options),
        Command::Init(options) => crate::init::run(options),
        Command::Secrets(options) => dotfiles_secrets::run(options).await,
        Command::Gpg(options) => dotfiles_secrets::run_gpg(options),
        Command::Switch(options) => crate::switch::run(options),
        Command::Update(options) => crate::update::run(options).map(|_outcome: UpdateOutcome| ()),
        Command::UpdateHistory(options) => crate::update_history::run(options),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

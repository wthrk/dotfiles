//! 利用者に公開する `dotfiles` コマンドの clap 定義。
//!
//! 環境変数で補える値は clap の `env` に寄せる。ここに置くサブコマンドは、ローカル flake の
//! 生成、更新、適用だけに限定し、リポジトリ保守用の操作は `xtask` 側へ分ける。

use clap::{Parser, Subcommand};

use crate::Result;

#[derive(Parser)]
#[command(name = "dotfiles")]
/// 利用者がローカル flake を作成、更新、適用するための最上位 CLI。
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
/// ローカル flake を作る操作、更新して適用する操作、そのまま適用する操作。
enum Command {
    Init(crate::init::InitOptions),
    Switch(crate::switch::SwitchOptions),
    Update(crate::update::UpdateOptions),
}

/// clap が確定したオプションだけを各処理へ渡し、ここでは実行ロジックを持たない。
pub(crate) fn dispatch() -> Result<()> {
    match Cli::parse().command {
        Command::Init(options) => crate::init::run(options),
        Command::Switch(options) => crate::switch::run(options),
        Command::Update(options) => crate::update::run(options),
    }
}

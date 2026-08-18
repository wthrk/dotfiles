//! 利用者に公開する `dotfiles` コマンドの clap 定義。
//!
//! 環境変数で補える値は clap の `env` に寄せる。ここに置くサブコマンドは利用者が直接
//! 実行する操作に限定し、リポジトリ保守用の操作は `xtask` 側へ分ける。

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
/// ローカル flake 操作、秘密情報復旧、設定適用を利用者向け command として公開する。
enum Command {
    Init(crate::init::InitOptions),
    Secrets(dotfiles_secrets::SecretsOptions),
    Gpg(dotfiles_secrets::GpgOptions),
    Switch(crate::switch::SwitchOptions),
    Update(crate::update::UpdateOptions),
    UpdateHistory(crate::update_history::UpdateHistoryOptions),
}

/// clap が確定したオプションだけを各処理へ渡す。
///
/// 同期 command は async runtime の外でそのまま実行し、runtime が必要な `Secrets` だけここで current-thread
/// runtime を立てる。`update-history` のような同期 command を async runtime 内で走らせると、内部で別 runtime を
/// ブリッジした際に drop 文脈が衝突するため、entrypoint 側でここを分ける。
pub(crate) fn dispatch() -> Result<()> {
    match Cli::parse().command {
        Command::Init(options) => crate::init::run(options),
        Command::Secrets(options) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(dotfiles_secrets::run(options)),
        Command::Gpg(options) => dotfiles_secrets::run_gpg(options),
        // 利用者が起動する `switch` は自分で中断できるため、外部コマンドに期限を置かない。適用した層は
        // 後始末を選ぶための戻り値であり、後始末を持たないこの経路では使わない。
        Command::Switch(options) => crate::switch::run(options, None).map(drop),
        Command::Update(options) => crate::update::run(options),
        Command::UpdateHistory(options) => crate::update_history::run(options),
    }
}

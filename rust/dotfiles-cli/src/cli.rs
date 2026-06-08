//! 利用者に公開する `dotfiles` コマンドの clap 定義。
//!
//! 環境変数で補える値は clap の `env` に寄せる。ここに置くサブコマンドは利用者が直接
//! 実行する操作に限定し、リポジトリ保守用の操作は `xtask` 側へ分ける。

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::Result;
use crate::update::{LOCK_CONTENDED_EXIT_CODE, UpdateOutcome};

/// `update` の通常成功（実適用 / up-to-date / commit 処理完了）に対応する exit code（0）。
const UPDATE_SUCCESS_EXIT_CODE: u8 = 0;
/// `update` の汎用失敗（network/nix 失敗等）に対応する exit code（1）。
const UPDATE_FAILURE_EXIT_CODE: u8 = 1;

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
/// 終了コードの分岐は `update` だけが持つ: lock 競合 skip（[`UpdateOutcome::LockContended`]）は zsh catch-up が
/// 「当日 marker を確定してよいか」を判別できるよう、exit 0（実適用/up-to-date）でも汎用失敗でもない専用 exit code
/// （[`LOCK_CONTENDED_EXIT_CODE`]）で返す（finding 3376248532）。他 command は従来どおり `Ok` → 0 / `Err` → 1。
pub(crate) async fn dispatch() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Ci(options) => crate::ci::run(options),
        Command::Init(options) => crate::init::run(options),
        Command::Secrets(options) => dotfiles_secrets::run(options).await,
        Command::Gpg(options) => dotfiles_secrets::run_gpg(options),
        Command::Switch(options) => crate::switch::run(options),
        // update は実行結果（適用/up-to-date か lock 競合 skip か）を exit code へ変換する。
        Command::Update(options) => return update_exit_code(crate::update::run(options)),
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

/// `update` の実行結果を exit code へ変換する。
///
/// `Completed`（実適用 / up-to-date 確認 / commit 処理）は 0、`LockContended`（別 update が lock 保持で skip）は
/// 専用 code（[`LOCK_CONTENDED_EXIT_CODE`]）、`Err`（network/nix 失敗等）は汎用失敗（1）。zsh は 0 のときだけ
/// 当日 catch-up marker を確定し、専用 code や失敗では確定しない（同日後続シェルの再試行を開けておく）。
fn update_exit_code(result: Result<UpdateOutcome>) -> ExitCode {
    if let Err(error) = &result {
        eprintln!("{error}");
    }
    ExitCode::from(update_exit_status(&result))
}

/// `update` 実行結果から exit code 数値を決める純粋関数（[`update_exit_code`] が `ExitCode` 変換と stderr 出力に使う）。
///
/// `Completed` → 成功（0）、`LockContended` → 専用 code（[`LOCK_CONTENDED_EXIT_CODE`] = 75）、`Err` → 汎用失敗（1）。
/// zsh catch-up が「当日 marker を確定してよいか」を 0 / 75 / 1 で識別する契約のため、各分岐の数値取り違え・退行を
/// I/O 無しで決定論的に固定できるよう、`ExitCode`（非 `PartialEq`）への変換と分離した純粋判定として切り出す。
fn update_exit_status(result: &Result<UpdateOutcome>) -> u8 {
    match result {
        Ok(UpdateOutcome::Completed) => UPDATE_SUCCESS_EXIT_CODE,
        Ok(UpdateOutcome::LockContended) => LOCK_CONTENDED_EXIT_CODE,
        Err(_) => UPDATE_FAILURE_EXIT_CODE,
    }
}

#[cfg(test)]
/// `update` 実行結果 → exit code 数値の対応を固定するテスト群。
///
/// zsh catch-up は 0（適用/up-to-date）・75（lock 競合 skip）・1（失敗）で marker 確定可否を分岐するため、
/// この対応の取り違えや退行が起きたら落ちることを保証する（I/O・network・nix 不要の決定論テスト）。
mod tests {
    use super::*;

    #[test]
    fn completed_maps_to_success_exit_code() {
        assert_eq!(
            update_exit_status(&Ok(UpdateOutcome::Completed)),
            UPDATE_SUCCESS_EXIT_CODE
        );
        assert_eq!(update_exit_status(&Ok(UpdateOutcome::Completed)), 0);
    }

    #[test]
    fn lock_contended_maps_to_dedicated_exit_code() {
        assert_eq!(
            update_exit_status(&Ok(UpdateOutcome::LockContended)),
            LOCK_CONTENDED_EXIT_CODE
        );
        // zsh 側が当日 marker を確定しない専用 code（75）であることを固定する。
        assert_eq!(update_exit_status(&Ok(UpdateOutcome::LockContended)), 75);
    }

    #[test]
    fn err_maps_to_generic_failure_exit_code() {
        let result: Result<UpdateOutcome> = Err(anyhow::anyhow!("network/nix 失敗を模す"));
        assert_eq!(update_exit_status(&result), UPDATE_FAILURE_EXIT_CODE);
        assert_eq!(update_exit_status(&result), 1);
    }

    #[test]
    fn success_lock_contended_and_failure_are_distinct() {
        // 3 経路の code が互いに取り違わない（混同退行を検知する）。
        let completed = update_exit_status(&Ok(UpdateOutcome::Completed));
        let contended = update_exit_status(&Ok(UpdateOutcome::LockContended));
        let failed = update_exit_status(&Err(anyhow::anyhow!("失敗")));
        assert_ne!(completed, contended);
        assert_ne!(completed, failed);
        assert_ne!(contended, failed);
    }
}

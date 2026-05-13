//! `cargo xtask check` から呼ばれる検証 CLI。
//!
//! この crate は xtask から起動される検証本体を持つ。main は引数を解釈して各検証 module へ
//! 委譲し、個別の検証手順は `static_checks`、`zsh`、`integration` に分ける。

use clap::{Parser, Subcommand};

mod command;
mod integration;
mod static_checks;
mod zsh;

use integration::RuntimeScenario;

type Result<T> = dotfiles_core::Result<T>;

#[derive(Parser)]
#[command(name = "dotfiles-checks")]
/// `cargo xtask check` から渡される検証グループ。
struct Cli {
    #[command(subcommand)]
    target: Option<CheckTarget>,
}

#[derive(Subcommand)]
/// VM なしで実行できる検証と、VM が必要な統合検証を分ける。
enum CheckTarget {
    Static,
    Zsh,
    Integration {
        #[arg(value_enum)]
        scenario: Option<RuntimeScenario>,
        #[arg(long, env = "DOTFILES_TEST_SOURCE_HASH")]
        source_hash: Option<String>,
    },
    All,
}

/// anyhow の失敗を標準エラーへ出し、xtask へ非 0 終了として返す。
fn main() -> std::process::ExitCode {
    match run(Cli::parse().target) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// 未指定時はローカル開発で毎回回す検証に絞り、VM 統合検証は明示指定時だけ動かす。
fn run(target: Option<CheckTarget>) -> Result<()> {
    match target {
        None => default_checks(),
        Some(CheckTarget::Static) => static_checks::check(),
        Some(CheckTarget::Zsh) => zsh::check(),
        Some(CheckTarget::All) => all_checks(),
        Some(CheckTarget::Integration {
            scenario,
            source_hash,
        }) => integration::run(scenario.unwrap_or(RuntimeScenario::Full), source_hash),
    }
}

/// 開発時の既定検証は、重い zsh 起動確認を含めず静的検証だけを行う。
fn default_checks() -> Result<()> {
    static_checks::check()
}

/// VM 内での初期導入シナリオまで含めて実行する。
fn all_checks() -> Result<()> {
    static_checks::check()?;
    zsh::check()?;
    integration::run(RuntimeScenario::Full, None)
}

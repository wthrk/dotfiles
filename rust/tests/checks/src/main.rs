//! `cargo xtask check` から呼ばれる検証 CLI。
//!
//! この crate は xtask から起動される検証本体を持つ。main は引数を解釈して各検証 module へ
//! 委譲し、個別の検証手順は `static_checks`、`test_checks`、`zsh`、`integration` に分ける。

use clap::{Parser, Subcommand};

mod command;
mod integration;
mod static_checks;
mod test_checks;
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
/// 静的検証、実行テスト、zsh、VM が必要な統合検証を分ける。
enum CheckTarget {
    Static,
    Test,
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
        Some(CheckTarget::Test) => test_checks::check(),
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

/// 静的検証・実行テスト・zsh・統合検証を独立に最後まで実行し、全結果を一度に返す。
/// 実機やBWSを直接起動する検証はこの集約器の責務に含めない。
fn all_checks() -> Result<()> {
    let mut failures = Vec::new();
    collect_check("static", static_checks::check(), &mut failures);
    collect_check("test", test_checks::check(), &mut failures);
    collect_check("zsh", zsh::check(), &mut failures);
    collect_check(
        "integration",
        integration::run(RuntimeScenario::Full, None),
        &mut failures,
    );

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("all checks failed: {}", failures.join("; "))
    }
}

fn collect_check(name: &str, result: Result<()>, failures: &mut Vec<String>) {
    if let Err(error) = result {
        eprintln!("check {name} failed: {error:#}");
        failures.push(format!("{name}: {error:#}"));
    }
}

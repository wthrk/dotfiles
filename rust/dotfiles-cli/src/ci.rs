//! リポジトリ CI（GitHub Actions）が叩く機械判定 command。
//!
//! nightly bump PR を無人 auto-merge してよいかを決める required status check `nightly-bump-guard` の
//! 実体をここに置く。判定ロジック自体は I/O を持たない [`bump_lock`] に閉じ、本 module は CI から
//! 「PR の base..head union 変更パス」「base / head の `flake.lock` 内容」を集めて純粋核へ渡す薄い層に
//! 限定する。shell の中に判定を再実装せず、Rust unit test で固定した規則を CI から呼ぶことで、guard の
//! fail-open を避ける。
//!
//! このサブコマンドはリポジトリ保守向けだが、利用者 CLI と同じ binary に載せるのは CI runner が dev shell
//! 経由で `dotfiles` を使えるためである。利用者向けの常用操作ではないが、xtask に置くと CI が xtask の
//! cargo ビルド経路に依存するため、判定核を持つ CLI 側に置いて test 可能性と再利用性を確保する。

mod bump_lock;

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Args, Subcommand};

use crate::Result;
use crate::process::run_capture;

#[derive(Args)]
/// CI 機械判定のサブコマンドをまとめる最上位 command。
pub(crate) struct CiOptions {
    #[command(subcommand)]
    command: CiCommand,
}

#[derive(Subcommand)]
/// 現在は nightly bump guard のみ。将来 CI 判定を足す場合もここに名詞で並べる。
enum CiCommand {
    VerifyBumpLock(VerifyBumpLockOptions),
}

#[derive(Args)]
/// nightly bump PR の変更パスと `flake.lock` 差分を許可規則に照らす option。
///
/// CI は PR の base SHA と head SHA を渡す。本 command は base..head の union 変更パスを `git diff` で集め、
/// base / head の `flake.lock` を `git show` で取り出して [`bump_lock::verify_bump`] に渡す。違反があれば
/// 非 0 終了し、required status check が fail する。
struct VerifyBumpLockOptions {
    /// PR の base commit SHA（マージ先の先端）。
    #[arg(long)]
    base: String,
    /// PR の head commit SHA（検査対象の先端）。
    #[arg(long)]
    head: String,
    /// 判定対象リポジトリの作業ツリー root（省略時はカレント）。
    #[arg(long)]
    repo: Option<PathBuf>,
}

/// CLI で parse 済みの `dotfiles ci` command を実行へ振り分ける。
pub(crate) fn run(options: CiOptions) -> Result<()> {
    match options.command {
        CiCommand::VerifyBumpLock(options) => run_verify_bump_lock(options),
    }
}

/// base..head の union 変更パスと両端の `flake.lock` を集め、bump guard 判定を実行する。
///
/// `git diff --name-only base..head` は base から head へ到達する全 commit の差分を union した変更パスを
/// 返す（各 commit 単独ではなく履歴全体）。これにより途中 commit で逸脱パスを足して head で消す回避を検出
/// できる。`flake.lock` 内容は `git show <sha>:flake.lock` で取り出す。判定核は [`bump_lock::verify_bump`]。
fn run_verify_bump_lock(options: VerifyBumpLockOptions) -> Result<()> {
    let git_dir_args: Vec<String> = match &options.repo {
        Some(repo) => vec!["-C".to_string(), repo.to_string_lossy().into_owned()],
        None => Vec::new(),
    };

    let range = format!("{}..{}", options.base, options.head);
    let diff_output = run_git(&git_dir_args, ["diff", "--name-only", &range])
        .context("collecting base..head union changed paths failed")?;
    let changed_paths: BTreeSet<String> = diff_output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();

    let old_lock = run_git(
        &git_dir_args,
        ["show", &format!("{}:flake.lock", options.base)],
    )
    .context("reading base flake.lock failed")?;
    let new_lock = run_git(
        &git_dir_args,
        ["show", &format!("{}:flake.lock", options.head)],
    )
    .context("reading head flake.lock failed")?;

    bump_lock::verify_bump(&changed_paths, &old_lock, &new_lock)?;
    println!(
        "nightly-bump-guard: OK ({} changed path(s))",
        changed_paths.len()
    );
    Ok(())
}

/// `git` をオプション付きで実行し stdout を返す。token 等の secret は引数に乗せない（git のみ）。
fn run_git<'a, I>(dir_args: &[String], subcommand: I) -> Result<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut args: Vec<std::ffi::OsString> = dir_args.iter().map(std::ffi::OsString::from).collect();
    args.extend(subcommand.into_iter().map(std::ffi::OsString::from));
    run_capture("git", args)
}

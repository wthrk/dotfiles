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
mod ruleset;

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, anyhow, bail};
use clap::{Args, Subcommand};
use serde_json::Value;

use crate::Result;
use crate::process::run_capture;

#[derive(Args)]
/// CI 機械判定のサブコマンドをまとめる最上位 command。
pub(crate) struct CiOptions {
    #[command(subcommand)]
    command: CiCommand,
}

#[derive(Subcommand)]
/// nightly bump guard と、適用済み ruleset の継続検証。将来 CI 判定を足す場合もここに名詞で並べる。
enum CiCommand {
    VerifyBumpLock(VerifyBumpLockOptions),
    VerifyRuleset(VerifyRulesetOptions),
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

#[derive(Args)]
/// 版管理された ruleset JSON が GitHub に正しく適用され続けているかを継続検証する option。
///
/// 版管理ファイル（`.github/rulesets/nightly-bump.json`）から ruleset `name` を読み、`gh api` で実適用済み
/// ruleset を取得して enforcement=active・bypass_actors 空・required check 包含を assert する。手動 `gh api`
/// 依存の適用が漏れ・改竄・context ドリフトしていれば fail し、required check の無効化（fail-open）を検知する。
struct VerifyRulesetOptions {
    /// 版管理された ruleset 定義 JSON のパス（`name` の解決に使う）。
    #[arg(long, default_value = ".github/rulesets/nightly-bump.json")]
    definition: PathBuf,
    /// 対象リポジトリ（`owner/repo`）。省略時は `GITHUB_REPOSITORY` 環境変数を使う。
    #[arg(long)]
    repository: Option<String>,
}

/// CLI で parse 済みの `dotfiles ci` command を実行へ振り分ける。
pub(crate) fn run(options: CiOptions) -> Result<()> {
    match options.command {
        CiCommand::VerifyBumpLock(options) => run_verify_bump_lock(options),
        CiCommand::VerifyRuleset(options) => run_verify_ruleset(options),
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

/// 版管理 ruleset 定義の `name` を解決し、実適用済み ruleset を取得して安全要件を継続検証する。
///
/// 手順: ① 版管理 JSON から ruleset `name` を読む → ② `gh api repos/{repo}/rulesets` で一覧から同名の
/// ruleset id を解決 → ③ `gh api repos/{repo}/rulesets/{id}` で詳細（`rules` 含む）を取得 → ④ I/O を持たない
/// 純粋核 [`ruleset::verify_applied_ruleset`] で enforcement=active・bypass 空・guard context 包含を assert。
///
/// token は `gh` が `GH_TOKEN` 環境変数から読むため argv に乗せず、本関数は token を一切扱わない（git/gh の
/// stdout だけを捌く）。最小権限読み取りで足りるか: ruleset 読み取りは repo administration:read を要する。CI の
/// `GITHUB_TOKEN` で読めない場合は `gh` が 403 で非 0 終了し、`run_capture` が fail として伝播する（fail-closed。
/// 読めない＝検証不能を success にしない）。その場合は admin 読み取り可能な token を CI に与える運用とする。
fn run_verify_ruleset(options: VerifyRulesetOptions) -> Result<()> {
    let repository = match options.repository {
        Some(repository) => repository,
        None => std::env::var("GITHUB_REPOSITORY").context(
            "repository not given and GITHUB_REPOSITORY is unset; cannot locate applied ruleset",
        )?,
    };

    let definition = std::fs::read_to_string(&options.definition).with_context(|| {
        format!(
            "reading ruleset definition {} failed",
            options.definition.display()
        )
    })?;
    let expected_name = serde_json::from_str::<Value>(&definition)
        .context("ruleset definition is not valid JSON")?
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("ruleset definition has no name field")?;

    let list = gh_api(&format!("repos/{repository}/rulesets"))
        .context("listing applied rulesets failed (need repo administration:read)")?;
    let ruleset_id = resolve_ruleset_id(&list, &expected_name)?;

    let applied = gh_api(&format!("repos/{repository}/rulesets/{ruleset_id}"))
        .context("fetching applied ruleset detail failed")?;
    ruleset::verify_applied_ruleset(&applied)?;

    println!(
        "nightly-bump ruleset `{expected_name}` (id {ruleset_id}) applied state OK \
         (active, bypass empty, guard required)"
    );
    Ok(())
}

/// ruleset 一覧 JSON から指定 `name` の ruleset id を解決する。0 件は適用漏れとして fail にする。
fn resolve_ruleset_id(list: &str, expected_name: &str) -> Result<i64> {
    let rulesets: Value = serde_json::from_str(list).context("ruleset list is not valid JSON")?;
    let rulesets = rulesets
        .as_array()
        .ok_or_else(|| anyhow!("ruleset list is not a JSON array"))?;

    let matched: Vec<&Value> = rulesets
        .iter()
        .filter(|ruleset| ruleset.get("name").and_then(Value::as_str) == Some(expected_name))
        .collect();
    match matched.as_slice() {
        [] => bail!(
            "no applied ruleset named `{expected_name}`; the versioned ruleset is not applied \
             to GitHub (fail-open: required check absent)"
        ),
        [ruleset] => ruleset
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("applied ruleset `{expected_name}` has no integer id")),
        _ => bail!(
            "multiple applied rulesets named `{expected_name}`; ambiguous which one enforces the \
             required check"
        ),
    }
}

/// `gh api <endpoint>` を実行し stdout（JSON）を返す。token は gh が env から読むため argv に乗せない。
fn gh_api(endpoint: &str) -> Result<String> {
    run_capture(
        "gh",
        ["api", endpoint].into_iter().map(std::ffi::OsString::from),
    )
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

#[cfg(test)]
mod tests {
    //! ruleset 一覧 JSON から name で id を解決する純粋部分（一致 1 件・0 件適用漏れ・複数件曖昧）を固定する。

    use super::resolve_ruleset_id;

    const LIST: &str = r#"[
      { "id": 11, "name": "other-ruleset" },
      { "id": 42, "name": "nightly-bump-protection" }
    ]"#;

    #[test]
    fn resolves_id_by_name() -> crate::Result<()> {
        assert_eq!(resolve_ruleset_id(LIST, "nightly-bump-protection")?, 42);
        Ok(())
    }

    #[test]
    fn fails_when_ruleset_not_applied() {
        let err = resolve_ruleset_id(LIST, "missing-ruleset").unwrap_err();
        assert!(err.to_string().contains("not applied"), "{err}");
    }

    #[test]
    fn fails_when_name_is_ambiguous() {
        let list = r#"[
          { "id": 1, "name": "dup" },
          { "id": 2, "name": "dup" }
        ]"#;
        let err = resolve_ruleset_id(list, "dup").unwrap_err();
        assert!(err.to_string().contains("multiple"), "{err}");
    }
}

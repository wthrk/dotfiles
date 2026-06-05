//! リポジトリ CI（GitHub Actions）が叩く機械判定 command。
//!
//! nightly bump PR を無人 auto-merge してよいかを決める required status check `nightly-bump-guard` の
//! 実体をここに置く。判定ロジック自体は I/O を持たない [`bump_lock`] に閉じ、本 module は CI から
//! 「PR の全 commit を union した変更パス」「base / head の `flake.lock` 内容」を集めて純粋核へ渡す薄い
//! 層に限定する。shell の中に判定を再実装せず、Rust unit test で固定した規則を CI から呼ぶことで、guard の
//! fail-open を避ける。
//!
//! 変更パス収集は `git diff --name-only base..head`（両端 tree の net 差分）ではなく
//! `git log --no-renames --name-only --pretty=format: base..head`（範囲内 **全 commit** の変更ファイル）を
//! union する。net diff は途中 commit で逸脱パスを add してから head までに remove すると取りこぼす
//! （add-then-remove）。全 commit union は中間 commit で一度でも触れたパスを必ず拾うため、これが一次防御で
//! ある（`--squash` マージ運用の有無に依存しない）。`--no-renames` は rename を delete+add の 2 件として
//! 扱わせ、宛先だけでなく**元パスも列挙**させる。これが無いと許可外の guard/ruleset/workflow を許可 prefix
//! 配下へ rename する PR が宛先（許可済み）だけ変更したように見えて guard を通過し、保護設定を削除・移動
//! できてしまう。元パスを union に入れることで rename 経由の許可外パス改変を取りこぼさない。
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
/// CI は PR の base SHA と head SHA を渡す。本 command は base..head の **全 commit** の変更パスを
/// `git log --name-only` で union 収集し、base / head の `flake.lock` を `git show` で取り出して
/// [`bump_lock::verify_bump`] に渡す。違反があれば非 0 終了し、required status check が fail する。
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

/// base..head の全 commit を union した変更パスと両端の `flake.lock` を集め、bump guard 判定を実行する。
///
/// `git log --name-only --pretty=format: base..head` は base..head に含まれる **各 commit** の変更ファイル名を
/// 列挙する。これを union する（[`collect_union_changed_paths`]）ことで、途中 commit で逸脱パスを追加し head
/// までに削除する add-then-remove を取りこぼさない。`git diff --name-only base..head`（両端 tree の net 差分）
/// では中間 commit の混入を検出できないため使わない。`flake.lock` 内容は `git show <sha>:flake.lock` で取り出す。
/// 判定核は [`bump_lock::verify_bump`]。
fn run_verify_bump_lock(options: VerifyBumpLockOptions) -> Result<()> {
    let git_dir_args: Vec<String> = match &options.repo {
        Some(repo) => vec!["-C".to_string(), repo.to_string_lossy().into_owned()],
        None => Vec::new(),
    };

    let range = format!("{}..{}", options.base, options.head);
    // `--pretty=format:` で commit ヘッダ行を空にし、`--name-only` の変更パス行だけを得る。範囲内の全 commit
    // を辿るため、各 commit が触れたパスがすべて出力に現れる（途中 commit で消えた net-clean なパスも含む）。
    //
    // `--no-renames` は必須である。これが無いと git は rename を 1 件の「rename」として扱い、`--name-only` は
    // **宛先パスのみ**を列挙する。すると guard/ruleset/workflow（許可外パス）を許可 prefix 配下（例
    // `docs/update-history/...`）へ rename する nightly PR が、許可済みの宛先だけを変更したように見えて guard を
    // 通過し、保護設定を削除・移動できてしまう。`--no-renames` で rename を delete+add の 2 件として扱わせると、
    // 元パス（許可外）も出力に現れ union へ入り、guard が fail する（許可外パスの混入を取りこぼさない）。
    let log_output = run_git(
        &git_dir_args,
        [
            "log",
            "--no-renames",
            "--name-only",
            "--pretty=format:",
            &range,
        ],
    )
    .context("collecting base..head per-commit changed paths failed")?;
    let changed_paths = collect_union_changed_paths(&log_output);

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

/// `git log --name-only --pretty=format: base..head` の出力を全 commit union の変更パス集合へ集約する。
///
/// 入力は範囲内 commit を順に並べた行で、`--pretty=format:` により commit ヘッダは空行になり、その後に当該
/// commit の変更ファイル名が 1 行 1 件で続く。複数 commit の出力が連結されるため、空行（commit 区切り兼ヘッダ）
/// を読み飛ばし、非空行を trim して `BTreeSet` で unique 化する。これにより、ある commit が追加し別の commit が
/// 削除して net 差分には現れないパスも、いずれかの commit に現れた時点で集合へ入る（add-then-remove の検出）。
///
/// この純粋関数として切り出すのは、実 git repo 無しで multi-commit 出力に対する union 化を unit test で固定する
/// ためである。git の実行（I/O）は caller が担い、本関数は文字列→集合の変換のみを行う。
fn collect_union_changed_paths(log_output: &str) -> BTreeSet<String> {
    log_output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
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
    //! ruleset 一覧 JSON から name で id を解決する純粋部分（一致 1 件・0 件適用漏れ・複数件曖昧）と、
    //! `git log --name-only` 出力を全 commit union の変更パスへ集約する純粋部分（add-then-remove を含む
    //! multi-commit ケースで逸脱パスを取りこぼさないこと）を固定する。

    use super::{collect_union_changed_paths, resolve_ruleset_id};

    #[test]
    fn union_collects_paths_across_all_commits() {
        // `--pretty=format:` で各 commit ヘッダは空行。2 commit ぶんの出力を連結したサンプル。
        let log_output = "\nflake.lock\ndocs/update-history/2026-06.toml\n\nflake.lock\n";
        let paths = collect_union_changed_paths(log_output);
        assert!(paths.contains("flake.lock"));
        assert!(paths.contains("docs/update-history/2026-06.toml"));
        // 空行（commit 区切り/ヘッダ）は集合に入らない。
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn union_detects_add_then_remove_dropped_by_net_diff() {
        // 中間 commit で逸脱パス（.github/workflows/x.yml）を add し、head までに remove したケース。
        // net diff（base..head 両端比較）では現れないが、commit 1 が触れているため union には残る。
        // commit 1: 逸脱パスを足す / commit 2: 逸脱パスを消し flake.lock だけ残す。
        let log_output =
            "\nflake.lock\n.github/workflows/x.yml\n\nflake.lock\n.github/workflows/x.yml\n";
        let paths = collect_union_changed_paths(log_output);
        assert!(
            paths.contains(".github/workflows/x.yml"),
            "add-then-remove path must survive union collection: {paths:?}"
        );
        // この集合を判定核へ渡せば逸脱パスとして fail する（union が一次防御であることの担保）。
        let err = super::bump_lock::verify_bump(&paths, MINIMAL_LOCK, MINIMAL_LOCK).unwrap_err();
        assert!(err.to_string().contains("disallowed path"), "{err}");
    }

    #[test]
    fn union_detects_disallowed_source_path_of_rename() {
        // rename を許可外 → 許可 prefix へ行う nightly PR の guard 回避を固定する。`--no-renames` を付けた
        // `git log --name-only` は rename を delete+add の 2 件として扱い、**元パスと宛先パスの両方**を行として
        // 出力する。元パスが許可外（ここでは `.github/rulesets/nightly-bump.json`）なら union に入って guard が
        // fail する。`--no-renames` を付けず宛先（`docs/update-history/nightly-bump.json`）のみが出る旧挙動だと
        // 許可 prefix 配下に見えて guard を通過してしまう。
        //
        // 1 commit ぶんの `--no-renames --name-only --pretty=format:` 出力（先頭空行=空 commit ヘッダ）。
        let log_output =
            "\ndocs/update-history/nightly-bump.json\n.github/rulesets/nightly-bump.json\n";
        let paths = collect_union_changed_paths(log_output);
        assert!(
            paths.contains(".github/rulesets/nightly-bump.json"),
            "rename source (disallowed) must survive union collection: {paths:?}"
        );
        // この集合を判定核へ渡すと許可外パスとして fail する（rename 回避を塞ぐことの担保）。
        let err = super::bump_lock::verify_bump(&paths, MINIMAL_LOCK, MINIMAL_LOCK).unwrap_err();
        assert!(err.to_string().contains("disallowed path"), "{err}");
    }

    /// add-then-remove ケースの判定確認に使う最小 lock（base=head 同一で lock 差分なし）。
    /// changed_paths だけが fail 要因になるよう、lock は無変更にしてある。
    const MINIMAL_LOCK: &str = r#"{
      "nodes": { "root": { "inputs": {} } },
      "root": "root",
      "version": 7
    }"#;

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

//! リポジトリ CI（GitHub Actions）が叩く機械判定 command。
//!
//! nightly bump PR を無人 auto-merge してよいかを決めるセキュリティチェック `verify-bump-lock` の実体を
//! ここに置く。`nightly-update.yml` の open-pr job が同一 run 内でこれをインライン実行し、合格時のみ PR head
//! へ `static checks` commit status を投稿して required check を満たす。判定ロジック自体は I/O を持たない
//! [`bump_lock`] に閉じ、本 module は CI から「PR の全 commit を union した変更パス」「base / head の
//! `flake.lock` 内容」を集めて純粋核へ渡す薄い層に限定する。shell の中に判定を再実装せず、Rust unit test で
//! 固定した規則を CI から呼ぶことで、gate の fail-open を避ける。
//!
//! 変更パス収集は `git diff --name-only base..head`（両端 tree の net 差分）ではなく
//! `git log --no-renames --name-only --pretty=format: base..head`（範囲内 **全 commit** の変更ファイル）を
//! union する。net diff は途中 commit で逸脱パスを add してから head までに remove すると取りこぼす
//! （add-then-remove）。全 commit union は中間 commit で一度でも触れたパスを必ず拾うため、これが一次防御で
//! ある（`--squash` マージ運用の有無に依存しない）。`--no-renames` は rename を delete+add の 2 件として
//! 扱わせ、宛先だけでなく**元パスも列挙**させる。これが無いと許可外の workflow / ソースを許可 prefix 配下へ
//! rename する PR が宛先（許可済み）だけ変更したように見えて gate を通過し、保護設定を削除・移動できてしまう。
//! 元パスを union に入れることで rename 経由の許可外パス改変を取りこぼさない。
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
/// nightly bump guard の機械判定。将来 CI 判定を足す場合もここに名詞で並べる。
enum CiCommand {
    VerifyBumpLock(VerifyBumpLockOptions),
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

/// CLI で parse 済みの `dotfiles ci` command を実行へ振り分ける。
pub(crate) fn run(options: CiOptions) -> Result<()> {
    match options.command {
        CiCommand::VerifyBumpLock(options) => run_verify_bump_lock(options),
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
        "verify-bump-lock: OK ({} changed path(s))",
        changed_paths.len()
    );
    Ok(())
}

/// `git log --name-only --pretty=format: base..head` の出力を全 commit union の変更パス集合へ集約する。
///
/// 入力は範囲内 commit を順に並べた行で、`--pretty=format:` により commit ヘッダは空行になり、その後に当該
/// commit の変更ファイル名が 1 行 1 件で続く。複数 commit の出力が連結されるため、空行（commit 区切り兼ヘッダ）
/// だけを読み飛ばし、それ以外の行は **改行のみを除いた生のパス** として `BTreeSet` で unique 化する。これに
/// より、ある commit が追加し別の commit が削除して net 差分には現れないパスも、いずれかの commit に現れた時点
/// で集合へ入る（add-then-remove の検出）。
///
/// パス行に `trim()` 等の正規化をかけないのは、auto-merge 一次防御の回避を塞ぐためである。許可判定（`flake.lock`
/// の厳密一致 / `docs/update-history/**` の prefix 一致）は guard が受け取った文字列そのものに対して行うので、
/// ここで前後空白を削ると、前後に空白を足した細工パス（例 `" flake.lock"`・`"docs/update-history/x ../evil "`）が
/// trim 後に許可と一致して見え、実際に変更されたパス（git が出力した生のパス）と検査対象がずれて gate をすり抜け
/// うる。`git log --name-only` は通常パスをそのまま出力し、特殊文字を含む場合のみ全体をダブルクォートで括った
/// octal escape 表現にする（その場合は `flake.lock` 等の許可 prefix と一致しない別文字列になる）。空白を含む
/// 許可外パスを許可と誤判定させないため、空行（改行のみ）判定以外の正規化を一切かけない。
///
/// この純粋関数として切り出すのは、実 git repo 無しで multi-commit 出力に対する union 化を unit test で固定する
/// ためである。git の実行（I/O）は caller が担い、本関数は文字列→集合の変換のみを行う。
fn collect_union_changed_paths(log_output: &str) -> BTreeSet<String> {
    log_output
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
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
    //! `git log --name-only` 出力を全 commit union の変更パスへ集約する純粋部分（add-then-remove を含む
    //! multi-commit ケースで逸脱パスを取りこぼさないこと）を固定する。

    use super::collect_union_changed_paths;

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
    fn union_keeps_genuine_allowed_paths_verbatim_and_passes_guard() -> crate::Result<()> {
        // 正当な nightly PR の出力（前後空白を含まない生パス）は許可 prefix と一致し guard を通る。
        // 集約が生パスを保つことを、許可判定（`verify_bump`）まで通して固定する。
        let log_output = "\nflake.lock\ndocs/update-history/2026-06.toml\n";
        let paths = collect_union_changed_paths(log_output);
        assert!(
            paths.contains("flake.lock"),
            "生の flake.lock を保持: {paths:?}"
        );
        assert!(
            paths.contains("docs/update-history/2026-06.toml"),
            "生の docs/update-history パスを保持: {paths:?}"
        );
        // base=head 同一 lock では rev 無変更で fail する（空 bump 防御）が、パスは許可判定を通る。
        // ここでは「パス判定が許可になること」を確かめるため、許可外パスではなく lock 側の理由で fail する
        // ことを確認する（disallowed path で fail しない）。
        let err = super::bump_lock::verify_bump(&paths, MINIMAL_LOCK, MINIMAL_LOCK).unwrap_err();
        assert!(
            !err.to_string().contains("disallowed path"),
            "正当な生パスは許可判定を通る（fail 理由は lock 側）: {err}"
        );
        Ok(())
    }

    #[test]
    fn union_does_not_normalize_whitespace_padded_crafted_path() {
        // trim 回避の退行固定: 前後空白を足した細工パスは、trim すれば許可 prefix と一致して見えるが、
        // 集約は改行のみを除いた生パスを保つため許可判定で許可外として fail する。leading-space 版の
        // `flake.lock` と trailing-space 版の `docs/update-history/...` の両方を 1 commit ぶんに混ぜる。
        let log_output = "\n flake.lock\ndocs/update-history/2026-06.toml \n";
        let paths = collect_union_changed_paths(log_output);
        // 生パス（空白付き）がそのまま集合に入る。trim 後の許可形には化けない。
        assert!(
            paths.contains(" flake.lock"),
            "leading space を保持: {paths:?}"
        );
        assert!(
            paths.contains("docs/update-history/2026-06.toml "),
            "trailing space を保持: {paths:?}"
        );
        assert!(
            !paths.contains("flake.lock"),
            "trim 済みの許可形へ正規化しない: {paths:?}"
        );
        // これらを判定核へ渡すと、空白付きの細工パスが許可外として fail する（一次防御がすり抜けない）。
        let err = super::bump_lock::verify_bump(&paths, MINIMAL_LOCK, MINIMAL_LOCK).unwrap_err();
        assert!(err.to_string().contains("disallowed path"), "{err}");
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
        // 出力する。元パスが許可外（ここでは `.github/workflows/nightly-update.yml`）なら union に入って guard が
        // fail する。`--no-renames` を付けず宛先（`docs/update-history/x.toml`）のみが出る旧挙動だと
        // 許可 prefix 配下に見えて guard を通過してしまう。
        //
        // 1 commit ぶんの `--no-renames --name-only --pretty=format:` 出力（先頭空行=空 commit ヘッダ）。
        let log_output = "\ndocs/update-history/x.toml\n.github/workflows/nightly-update.yml\n";
        let paths = collect_union_changed_paths(log_output);
        assert!(
            paths.contains(".github/workflows/nightly-update.yml"),
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
}

//! nightly bump PR を無人 auto-merge してよいかを機械判定する純粋規則。
//!
//! この module は I/O を持たず、CI が収集した「PR の全 commit を base..head で union した変更パス集合」と
//! 「base / head の `flake.lock` 内容」を受け取り、許可された bump だけかを決める。判定ロジックを CLI
//! （`dotfiles ci verify-bump-lock`）の純粋核に置くことで、required status check `nightly-bump-guard` の
//! 実体を Rust unit test で固定し、shell の中で再実装しない。
//!
//! 判定する不変条件は 2 つだけである。
//!
//! 1. **変更パス限定**: PR が触ってよいのは `flake.lock` と `docs/update-history/**` だけ。`.github/**`、
//!    ruleset 定義、ソース、その他いずれかが base..head の union 差分に 1 つでも混ざれば fail。これにより
//!    nightly PR が guard / ruleset / workflow / コードを自己改変して無人で main へ入れる経路を塞ぐ。
//!    PR の各 commit ではなく base..head の union を検査するのは、途中 commit で逸脱パスを足して最終 head で
//!    消す回避（diff は綺麗だが履歴は汚い）を防ぐためである。union パス集合の収集は CLI 側（`git diff`/`git
//!    log` 由来）の責務で、本 module は与えられた集合を判定する。
//!
//! 2. **lock 差分限定**: `flake.lock` の差分は許可 input 集合の **rev 変更のみ**。許可集合は nightly が
//!    bump する input（nixpkgs + tap 4 本）を owner/repo の厳密一致で列挙する。framework input（nix-darwin /
//!    home-manager / nix-homebrew）の rev 変更、想定外 input の追加・削除、source（owner/repo/type/ref/url）の
//!    改変はすべて fail。version / root / 既存 input の source 同一性も検査し、lock 全体が「許可 input の rev
//!    だけが動いた」状態であることを確認する。
//!
//! どちらかに違反すれば [`verify_bump`] は違反理由を載せた `Err` を返し、CLI は非 0 で終了する。required
//! status check はこの非 0 終了で fail し、ruleset（bypass actors 空）が auto-merge を止める。

use std::collections::BTreeSet;

use anyhow::{Context, anyhow, bail};
use serde_json::Value;

use crate::Result;

/// nightly bump が触ってよい唯一のファイル（`docs/update-history/**` は prefix 判定）。
const ALLOWED_LOCK_PATH: &str = "flake.lock";
/// nightly が記録を追記してよい履歴ディレクトリ prefix。
const ALLOWED_HISTORY_PREFIX: &str = "docs/update-history/";

/// nightly が rev を bump してよい input の許可集合（lock node の input 名 → 期待 owner/repo）。
///
/// owner/repo を厳密一致で固定し、prefix 一致や input 名だけの一致では許可しない。nightly は nixpkgs と
/// brew tap 4 本だけを bump する。framework input（darwin / home-manager / nix-homebrew）と brew-src は
/// この集合に含めないため、それらの rev が動けば未許可変更として fail する。
const ALLOWED_BUMP_INPUTS: [(&str, &str, &str); 5] = [
    ("nixpkgs", "NixOS", "nixpkgs"),
    ("homebrew-homebrew-core", "homebrew", "homebrew-core"),
    ("homebrew-homebrew-cask", "homebrew", "homebrew-cask"),
    ("homebrew-azure-bicep", "Azure", "homebrew-bicep"),
    ("homebrew-hashicorp-tap", "hashicorp", "homebrew-tap"),
];

/// 1 つの lock node の同一性を決める source 座標（rev を除く）。
///
/// rev だけが変わったことを確かめるため、rev 以外（owner / repo / type / ref / url / flake フラグ）を
/// まとめて比較する。許可 input はこの座標が完全一致した上で rev だけ異なることを要求し、未許可 input は
/// rev を含む全フィールドが一致することを要求する。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceCoords {
    owner: Option<String>,
    repo: Option<String>,
    node_type: Option<String>,
    reference: Option<String>,
    url: Option<String>,
    flake: Option<bool>,
}

/// 与えられた変更パス集合と base/head の `flake.lock` 内容から、無人 auto-merge 可否を判定する。
///
/// `changed_paths` は PR の base..head union 変更パス（repo ルート相対、`/` 区切り）。`old_lock` / `new_lock`
/// は base / head の `flake.lock` の生 JSON。許可外パス・許可外 lock 差分があれば、最初に見つけた違反理由を
/// 載せた `Err` を返す。すべて許可範囲内なら `Ok(())`。CLI はこの結果で終了コードを決める。
///
/// caller responsibility: `changed_paths` は base..head の union（途中 commit を含む）であること。各 commit
/// 単位の差分や head 単独の差分を渡すと、途中 commit 混入の検出という不変条件が崩れる。
pub(crate) fn verify_bump(
    changed_paths: &BTreeSet<String>,
    old_lock: &str,
    new_lock: &str,
) -> Result<()> {
    verify_changed_paths(changed_paths)?;
    verify_lock_diff(old_lock, new_lock)?;
    Ok(())
}

/// 変更パス集合が `flake.lock` と `docs/update-history/**` だけかを検査する。
fn verify_changed_paths(changed_paths: &BTreeSet<String>) -> Result<()> {
    for path in changed_paths {
        if path == ALLOWED_LOCK_PATH || path.starts_with(ALLOWED_HISTORY_PREFIX) {
            continue;
        }
        bail!(
            "disallowed path in nightly bump PR: {path} \
             (allowed: {ALLOWED_LOCK_PATH}, {ALLOWED_HISTORY_PREFIX}**)"
        );
    }
    Ok(())
}

/// `flake.lock` の base→head 差分が「許可 input の rev 変更だけ」かを検査する。
fn verify_lock_diff(old_lock: &str, new_lock: &str) -> Result<()> {
    let old: Value = serde_json::from_str(old_lock).context("base flake.lock is not valid JSON")?;
    let new: Value = serde_json::from_str(new_lock).context("head flake.lock is not valid JSON")?;

    // version と root（input ワイヤリング）は不変でなければならない。これらが変われば input の追加・削除や
    // lock フォーマット変更を意味し、rev bump の範囲を超える。
    if old.get("version") != new.get("version") {
        bail!("flake.lock version changed; nightly bump must not change lock format");
    }
    if old.get("root") != new.get("root") {
        bail!("flake.lock root inputs wiring changed; nightly bump must not add/remove inputs");
    }

    let old_nodes = nodes(&old, "base")?;
    let new_nodes = nodes(&new, "head")?;

    // node 集合（input 名）は完全一致でなければならない。追加・削除はいずれも未許可。
    let old_names: BTreeSet<&str> = old_nodes.keys().map(|k| k.as_str()).collect();
    let new_names: BTreeSet<&str> = new_nodes.keys().map(|k| k.as_str()).collect();
    if old_names != new_names {
        let added: Vec<&&str> = new_names.difference(&old_names).collect();
        let removed: Vec<&&str> = old_names.difference(&new_names).collect();
        bail!(
            "flake.lock node set changed (added: {added:?}, removed: {removed:?}); \
             nightly bump must not add or remove inputs"
        );
    }

    for (name, old_node) in old_nodes {
        // node 名集合の一致は直前に検査済みのため `get` は必ず `Some`。安全側として欠落時も fail にする。
        let Some(new_node) = new_nodes.get(name) else {
            bail!("flake.lock node `{name}` missing on head despite equal node set");
        };
        verify_node(name, old_node, new_node)?;
    }
    Ok(())
}

/// `flake.lock` の `nodes` オブジェクトを取り出す。
fn nodes<'a>(lock: &'a Value, label: &str) -> Result<&'a serde_json::Map<String, Value>> {
    lock.get("nodes")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("{label} flake.lock has no nodes object"))
}

/// 単一 node の base→head 差分を許可規則に照らす。
///
/// 許可 input は source 座標（rev 以外）一致を要求し、rev は変わってよい。未許可 input（framework など）は
/// rev を含め全フィールド一致を要求する。`locked` を持たない node（`root`）は source 座標を持たないため、
/// 既に同一性が確認済みの前提でそのまま許可する。
fn verify_node(name: &str, old_node: &Value, new_node: &Value) -> Result<()> {
    let old_locked = old_node.get("locked");
    let new_locked = new_node.get("locked");

    // locked を持たない node（root 等）は version/root 検査側で扱う。source 座標が無いものは
    // node 同士を厳密一致で比較し、いかなる差分も未許可とする。
    let (Some(old_locked), Some(new_locked)) = (old_locked, new_locked) else {
        if old_node != new_node {
            bail!("flake.lock node `{name}` changed but has no locked source; not an allowed bump");
        }
        return Ok(());
    };

    let old_coords = source_coords(old_locked);
    let new_coords = source_coords(new_locked);

    // original（input が宣言する原座標）は rev bump で変わらない。改変されれば input の差し替え。
    if old_node.get("original") != new_node.get("original") {
        bail!("flake.lock node `{name}` original source declaration changed; not an allowed bump");
    }
    // inputs（node 間ワイヤリング）も rev bump では変わらない。
    if old_node.get("inputs") != new_node.get("inputs") {
        bail!("flake.lock node `{name}` input wiring changed; not an allowed bump");
    }

    if let Some((_, owner, repo)) = ALLOWED_BUMP_INPUTS.iter().find(|(n, _, _)| *n == name) {
        // 許可 input: source 座標（rev 以外）一致 + owner/repo が期待値に厳密一致。rev は変わってよい。
        if old_coords != new_coords {
            bail!(
                "flake.lock node `{name}` source coordinates changed (not just rev); not an allowed bump"
            );
        }
        match (new_coords.owner.as_deref(), new_coords.repo.as_deref()) {
            (Some(o), Some(r)) if o == *owner && r == *repo => Ok(()),
            other => bail!(
                "flake.lock node `{name}` owner/repo {other:?} does not match \
                 allowed {owner}/{repo}; not an allowed bump"
            ),
        }
    } else {
        // 未許可 input（framework / brew-src など）: rev を含め locked 全体が不変でなければならない。
        if old_locked != new_locked {
            bail!(
                "flake.lock node `{name}` is not in the allowed bump set but its locked source changed \
                 (e.g. framework input rev); not an allowed bump"
            );
        }
        Ok(())
    }
}

/// `locked` オブジェクトから rev を除いた source 座標を取り出す。
fn source_coords(locked: &Value) -> SourceCoords {
    let str_field = |key: &str| locked.get(key).and_then(Value::as_str).map(str::to_string);
    SourceCoords {
        owner: str_field("owner"),
        repo: str_field("repo"),
        node_type: str_field("type"),
        reference: str_field("ref"),
        url: str_field("url"),
        flake: locked.get("flake").and_then(Value::as_bool),
    }
}

#[cfg(test)]
mod tests {
    //! 許可パス限定と「許可 input の rev だけが動いた lock」判定を固定する。許可外パス・input 追加削除・
    //! framework rev 変更・source 座標改変・owner/repo すり替えがすべて fail することを確認する。

    use super::*;

    fn paths(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// 最小構成の lock（nixpkgs 許可 input 1 つ + framework input 1 つ）。`{rev}` を差し替えて使う。
    fn lock_with(nixpkgs_rev: &str, darwin_rev: &str) -> String {
        format!(
            r#"{{
  "nodes": {{
    "darwin": {{
      "inputs": {{ "nixpkgs": ["nixpkgs"] }},
      "locked": {{ "owner": "LnL7", "repo": "nix-darwin", "rev": "{darwin_rev}", "type": "github" }},
      "original": {{ "owner": "LnL7", "repo": "nix-darwin", "type": "github" }}
    }},
    "nixpkgs": {{
      "locked": {{ "owner": "NixOS", "repo": "nixpkgs", "rev": "{nixpkgs_rev}", "type": "github" }},
      "original": {{ "owner": "NixOS", "ref": "nixpkgs-unstable", "repo": "nixpkgs", "type": "github" }}
    }},
    "root": {{ "inputs": {{ "darwin": "darwin", "nixpkgs": "nixpkgs" }} }}
  }},
  "root": "root",
  "version": 7
}}"#
        )
    }

    #[test]
    fn accepts_allowed_paths() -> Result<()> {
        verify_changed_paths(&paths(&[
            "flake.lock",
            "docs/update-history/2026-06.toml",
            "docs/update-history/2026-07.toml",
        ]))
    }

    #[test]
    fn rejects_workflow_path() {
        let err = verify_changed_paths(&paths(&[
            "flake.lock",
            ".github/workflows/nightly-update.yml",
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("disallowed path"), "{err}");
    }

    #[test]
    fn rejects_ruleset_and_source_paths() {
        assert!(verify_changed_paths(&paths(&[".github/rulesets/nightly.json"])).is_err());
        assert!(verify_changed_paths(&paths(&["rust/dotfiles-cli/src/ci.rs"])).is_err());
        // docs/update-history directory 自体（末尾 / 無し）は prefix にマッチしないため未許可。
        assert!(verify_changed_paths(&paths(&["docs/update-history"])).is_err());
    }

    #[test]
    fn accepts_nixpkgs_rev_bump_only() -> Result<()> {
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd");
        verify_bump(&paths(&["flake.lock"]), &old, &new)
    }

    #[test]
    fn rejects_framework_rev_bump() {
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("aaaa", "eeee");
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("not in the allowed bump set"),
            "{err}"
        );
    }

    #[test]
    fn rejects_unexpected_input_addition() {
        let old = lock_with("aaaa", "dddd");
        // head に未宣言 input を 1 つ足す。
        let new = old.replace(
            r#""root": {"#,
            r#""evil": { "locked": { "owner": "x", "repo": "y", "rev": "1", "type": "github" }, "original": {} },
    "root": {"#,
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("node set changed"), "{err}");
    }

    #[test]
    fn rejects_source_url_change_on_allowed_input() {
        let old = lock_with("aaaa", "dddd");
        // nixpkgs の owner を別 fork へ差し替える（rev も動かす）。
        let new = lock_with("bbbb", "dddd").replace(
            r#""owner": "NixOS", "repo": "nixpkgs", "rev": "bbbb""#,
            r#""owner": "evil", "repo": "nixpkgs", "rev": "bbbb""#,
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("source coordinates changed")
                || err.to_string().contains("owner/repo"),
            "{err}"
        );
    }

    #[test]
    fn rejects_original_declaration_change() {
        let old = lock_with("aaaa", "dddd");
        // nixpkgs の original ref を差し替える。
        let new = lock_with("bbbb", "dddd")
            .replace(r#""ref": "nixpkgs-unstable""#, r#""ref": "nixos-25.05""#);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string()
                .contains("original source declaration changed"),
            "{err}"
        );
    }

    #[test]
    fn rejects_version_change() {
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd").replace(r#""version": 7"#, r#""version": 8"#);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("version changed"), "{err}");
    }

    #[test]
    fn rejects_node_inputs_wiring_change() {
        // 許可 input（nixpkgs）の rev は動いてよいが、node 間 inputs ワイヤリング改変は rev bump 範囲外。
        // darwin の inputs.nixpkgs follows を別 node 名へ差し替える（rev は据え置き）。
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd").replace(
            r#""inputs": { "nixpkgs": ["nixpkgs"] }"#,
            r#""inputs": { "nixpkgs": ["evil"] }"#,
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("input wiring changed"), "{err}");
    }

    #[test]
    fn rejects_node_deletion() {
        // head で許可 input（nixpkgs）node を削除する。node 集合不一致として fail（追加と同様に削除も未許可）。
        let old = lock_with("aaaa", "dddd");
        // nixpkgs node ブロックと root の参照を削り、node 集合を変える。
        let new = r#"{
  "nodes": {
    "darwin": {
      "inputs": { "nixpkgs": ["nixpkgs"] },
      "locked": { "owner": "LnL7", "repo": "nix-darwin", "rev": "dddd", "type": "github" },
      "original": { "owner": "LnL7", "repo": "nix-darwin", "type": "github" }
    },
    "root": { "inputs": { "darwin": "darwin" } }
  },
  "root": "root",
  "version": 7
}"#
        .to_string();
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("node set changed"), "{err}");
    }

    #[test]
    fn rejects_no_change_is_fine_but_disallowed_path_fails_even_with_clean_lock() {
        // lock が無変更でも、union に逸脱パスが 1 つあれば fail（途中 commit 混入の検出）。
        let lock = lock_with("aaaa", "dddd");
        let err = verify_bump(
            &paths(&["flake.lock", ".github/workflows/x.yml"]),
            &lock,
            &lock,
        )
        .unwrap_err();
        assert!(err.to_string().contains("disallowed path"), "{err}");
    }
}

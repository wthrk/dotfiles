//! 宣言パッケージ版の `nix eval` 実行と、評価時 meta/src からの owner/repo・changelog・homepage 導出。
//!
//! nightly の nix 版差分は、各宣言パッケージの `pname`/`version` と当該版リリースノート取得元（owner/repo・
//! changelog・homepage）を要する。これらは評価時属性のみで数秒・ビルド/フェッチ非実行で取れる。本 module は
//! [`eval_declared_versions`] で `nix eval` を **生の評価値だけ返す最小 `--apply` 式**（owner/repo 導出規則を
//! 含まない）で起動し、整形・repo 導出・changelog/homepage 抽出を **Rust 側**で行って [`NixPackage`] へ畳む
//! （旧 `eval-declared-versions.sh` + `derive-repo.nix` の置き換え）。
//!
//! repo（owner/repo）導出の優先: ①`meta.homepage` が github ②無ければ `src`（owner+repo 直接、無ければ
//! url/urls の github URL）③無ければ `meta.changelog` の github URL。github 由来が取れなければ空（version-only
//! 行き）。すべて信頼境界外の値であり、実取得時に host allowlist で機械検証する。
//!
//! flake.lock の rev 抽出（[`lock_node_rev`]）も serde_json で行い、workflow の jq を撤去する。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::diff::NixPackage;
use crate::Result;
use crate::process::run_capture;

/// 宣言パッケージ list を name→生評価値へ畳む最小 `--apply` 式（owner/repo 導出規則は持たない）。
///
/// 評価時属性（`pname`/`version`/`meta.changelog`/`meta.homepage`/`src.owner`/`src.repo`/`src.url`/`src.urls`）を
/// そのまま JSON object へ写すだけ。整形と owner/repo 導出は Rust 側（[`derive_package`]）が行う。`src` の各
/// フィールドは存在しないパッケージがあるため `or` で握り潰す。`builtins.parseDrvName` は pname 欠落時の名前
/// フォールバック。
const RAW_EVAL_APPLY: &str = r#"ps: builtins.listToAttrs (map (p: {
  name = p.pname or (builtins.parseDrvName (p.name or "")).name;
  value = {
    version = p.version or "";
    homepage = p.meta.homepage or null;
    changelog = p.meta.changelog or null;
    src_owner = p.src.owner or null;
    src_repo = p.src.repo or null;
    src_url = p.src.url or null;
    src_urls = p.src.urls or null;
  };
}) ps)"#;

/// 1 パッケージの生評価値（`nix eval` が返す未整形 JSON）。
///
/// `homepage`/`changelog`/`src_*` は文字列・list・null・非文字列が混在しうる（nixpkgs の `meta.*` は複数 URL の
/// list になり得る）。型は `serde_json::Value` で受け、Rust 側で文字列正規化する（list/非文字列は空へ倒し JSON
/// スキーマを壊さない）。
#[derive(Debug, Deserialize)]
struct RawPackage {
    #[serde(default)]
    version: String,
    #[serde(default)]
    homepage: serde_json::Value,
    #[serde(default)]
    changelog: serde_json::Value,
    #[serde(default)]
    src_owner: serde_json::Value,
    #[serde(default)]
    src_repo: serde_json::Value,
    #[serde(default)]
    src_url: serde_json::Value,
    #[serde(default)]
    src_urls: serde_json::Value,
}

/// `nix eval` で参照構成の宣言パッケージ（home.packages + environment.systemPackages）を評価し、
/// name→[`NixPackage`]（version + 導出済み repo/changelog/homepage）へ畳む。
///
/// home と system を統合し、同名は system 側を優先する（実フリートでは重複は基本起きない）。eval はビルド/
/// フェッチを走らせず数秒で完了する。owner/repo 導出と文字列正規化は Rust（[`derive_package`]）の責務。
pub(crate) fn eval_declared_versions(reference: &str) -> Result<BTreeMap<String, NixPackage>> {
    let user = run_capture(
        "nix",
        [
            "eval".into(),
            "--raw".into(),
            format!(".#{reference}.config.system.primaryUser").into(),
        ],
    )?;
    let user = user.trim();
    let home = eval_package_list(&format!(
        ".#{reference}.config.home-manager.users.{user}.home.packages"
    ))?;
    let system = eval_package_list(&format!(".#{reference}.config.environment.systemPackages"))?;
    let mut merged = home;
    merged.extend(system);
    Ok(merged)
}

/// 1 つのパッケージ list attribute を `nix eval --json --apply` で評価し、導出済み name→[`NixPackage`] を返す。
fn eval_package_list(attr: &str) -> Result<BTreeMap<String, NixPackage>> {
    let json = run_capture(
        "nix",
        [
            "eval".into(),
            "--json".into(),
            attr.into(),
            "--apply".into(),
            RAW_EVAL_APPLY.into(),
        ],
    )?;
    let raw: BTreeMap<String, RawPackage> = serde_json::from_str(&json)?;
    Ok(raw
        .into_iter()
        .map(|(name, package)| (name, derive_package(package)))
        .collect())
}

/// 生評価値 1 件を導出済み [`NixPackage`]（version + repo/changelog/homepage）へ翻訳する純粋関数。
fn derive_package(raw: RawPackage) -> NixPackage {
    let homepage = as_str(&raw.homepage);
    let changelog = as_str(&raw.changelog);
    NixPackage {
        version: raw.version,
        repo: derive_repo(
            &homepage,
            &as_str(&raw.src_owner),
            &as_str(&raw.src_repo),
            &as_str(&raw.src_url),
            first_url(&raw.src_urls),
            &changelog,
        ),
        // changelog（無ければ homepage）を Releases API 空振り時の raw フォールバック取得元にする。
        notes_source: if changelog.is_empty() {
            homepage.clone()
        } else {
            changelog
        },
        homepage,
    }
}

/// owner/repo を homepage(github) → src → changelog(github) の優先で導出する純粋関数（取れなければ空文字）。
fn derive_repo(
    homepage: &str,
    src_owner: &str,
    src_repo: &str,
    src_url: &str,
    src_first_url: Option<&str>,
    changelog: &str,
) -> String {
    if let Some(repo) = repo_from_url(homepage) {
        return repo;
    }
    if !src_owner.is_empty() && !src_repo.is_empty() {
        return format!("{src_owner}/{src_repo}");
    }
    if let Some(repo) = repo_from_url(src_url) {
        return repo;
    }
    if let Some(repo) = src_first_url.and_then(repo_from_url) {
        return repo;
    }
    repo_from_url(changelog).unwrap_or_default()
}

/// github URL 文字列から `owner/repo` を取り出す純粋関数（末尾 `.git`・クエリ/フラグメントは除く）。
fn repo_from_url(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let mut segments = rest.split('/');
    let owner = segments.next().filter(|s| !s.is_empty())?;
    let repo_raw = segments.next().filter(|s| !s.is_empty())?;
    // 末尾のクエリ/フラグメント（`?`/`#` 以降）を落とし、続く `.git` を剥がす。
    let repo = repo_raw
        .split(['?', '#'])
        .next()
        .unwrap_or(repo_raw)
        .trim_end_matches(".git");
    if repo.is_empty() {
        None
    } else {
        Some(format!("{owner}/{repo}"))
    }
}

/// `serde_json::Value` を文字列へ正規化する（文字列はそのまま、list/null/非文字列は空文字）。
///
/// nixpkgs の `meta.changelog`/`meta.homepage` は複数 URL の list になり得る。list/非文字列をそのまま運ぶと
/// 下流の文字列前提が壊れるため空文字へ倒す（JSON スキーマ安定）。
fn as_str(value: &serde_json::Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

/// `src.urls`（list）の先頭文字列を返す純粋関数（list でない/空なら `None`）。
fn first_url(value: &serde_json::Value) -> Option<&str> {
    value.as_array()?.first()?.as_str()
}

/// flake.lock の `nodes.<node>.locked.rev` を serde_json で取り出す（workflow の jq 置き換え）。
pub(crate) fn lock_node_rev(lock_path: &Path, node: &str) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct Lock {
        #[serde(default)]
        nodes: BTreeMap<String, Node>,
    }
    #[derive(Deserialize)]
    struct Node {
        locked: Option<Locked>,
    }
    #[derive(Deserialize)]
    struct Locked {
        rev: Option<String>,
    }
    let text = std::fs::read_to_string(lock_path)?;
    let lock: Lock = serde_json::from_str(&text)?;
    Ok(lock
        .nodes
        .get(node)
        .and_then(|node| node.locked.as_ref())
        .and_then(|locked| locked.rev.clone()))
}

#[cfg(test)]
mod tests {
    //! owner/repo 導出の 4 分岐（homepage→src→changelog・`.git` 剥がし・非 github 空）・文字列正規化
    //! （list/非文字列→空）・flake.lock rev 抽出を network/nix 抜きで固定する。

    use super::*;

    #[test]
    fn derive_repo_prefers_homepage_then_src_then_changelog() {
        // ① homepage が github → そこから（src/changelog より優先）。
        assert_eq!(
            derive_repo(
                "https://github.com/neovim/neovim",
                "src-owner",
                "src-repo",
                "",
                None,
                "https://github.com/cl-owner/cl-repo",
            ),
            "neovim/neovim"
        );
        // ② homepage 非 github → src の owner+repo。
        assert_eq!(
            derive_repo(
                "https://example.com/home",
                "BurntSushi",
                "ripgrep",
                "",
                None,
                "",
            ),
            "BurntSushi/ripgrep"
        );
        // ②' src owner+repo 無し → src.url の github。
        assert_eq!(
            derive_repo("", "", "", "https://github.com/o/r", None, ""),
            "o/r"
        );
        // ②'' src.url 無し → src.urls 先頭の github。
        assert_eq!(
            derive_repo("", "", "", "", Some("https://github.com/o/r2"), ""),
            "o/r2"
        );
        // ③ homepage/src 無し → changelog の github、末尾 `.git` 剥がし。
        assert_eq!(
            derive_repo("", "", "", "", None, "https://github.com/owner/proj.git"),
            "owner/proj"
        );
        // ④ いずれも非 github → 空文字。
        assert_eq!(
            derive_repo(
                "https://gitlab.com/o/r",
                "",
                "",
                "",
                None,
                "https://example.com/changelog",
            ),
            ""
        );
    }

    #[test]
    fn repo_from_url_strips_git_and_query() {
        assert_eq!(
            repo_from_url("https://github.com/o/r.git").as_deref(),
            Some("o/r")
        );
        assert_eq!(
            repo_from_url("https://github.com/o/r/releases/tag/v1").as_deref(),
            Some("o/r")
        );
        assert_eq!(
            repo_from_url("https://github.com/o/r?tab=x").as_deref(),
            Some("o/r")
        );
        assert_eq!(repo_from_url("https://gitlab.com/o/r"), None);
        assert_eq!(repo_from_url("not a url"), None);
        assert_eq!(repo_from_url("https://github.com/o"), None);
    }

    #[test]
    fn as_str_normalizes_list_and_nonstring_to_empty() {
        // changelog/homepage が list なら空文字へ（JSON スキーマ安定 = 下流 deserialize 不落）。
        assert_eq!(
            as_str(&serde_json::json!([
                "https://github.com/o/r/blob/main/A.md",
                "https://github.com/o/r/blob/main/B.md"
            ])),
            ""
        );
        assert_eq!(
            as_str(&serde_json::json!(
                "https://github.com/o/r/blob/main/CHANGELOG.md"
            )),
            "https://github.com/o/r/blob/main/CHANGELOG.md"
        );
        assert_eq!(as_str(&serde_json::Value::Null), "");
        assert_eq!(as_str(&serde_json::json!(42)), "");
    }

    #[test]
    fn derive_package_falls_back_changelog_to_homepage_for_notes_source() {
        let raw = RawPackage {
            version: "1.2.3".to_string(),
            homepage: serde_json::json!("https://homepage.example/"),
            changelog: serde_json::Value::Null,
            src_owner: serde_json::Value::Null,
            src_repo: serde_json::Value::Null,
            src_url: serde_json::Value::Null,
            src_urls: serde_json::Value::Null,
        };
        let package = derive_package(raw);
        assert_eq!(package.version, "1.2.3");
        // changelog 無し → notes_source は homepage へフォールバック。
        assert_eq!(package.notes_source, "https://homepage.example/");
        assert_eq!(package.homepage, "https://homepage.example/");
    }

    #[test]
    fn lock_node_rev_extracts_locked_rev() -> Result<()> {
        let mut path = std::env::temp_dir();
        path.push(format!("dotfiles-uh-lock-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"nodes":{"nixpkgs":{"locked":{"rev":"deadbeef"}},"root":{}}}"#,
        )?;
        assert_eq!(
            lock_node_rev(&path, "nixpkgs")?.as_deref(),
            Some("deadbeef")
        );
        assert_eq!(lock_node_rev(&path, "missing")?, None);
        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}

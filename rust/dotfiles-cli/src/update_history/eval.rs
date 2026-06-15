//! 宣言パッケージ版の `nix eval` 実行と、評価時 meta/src からの owner/repo・changelog・homepage 導出。
//!
//! nightly の nix 版差分は、各宣言パッケージの `pname`/`version` と当該版リリースノート取得元（owner/repo・
//! changelog・homepage）を要する。これらは評価時属性のみで数秒・ビルド/フェッチ非実行で取れる。本 module は
//! [`eval_declared_versions`] で `nix eval` を **生の評価値だけ返す最小 `--apply` 式**（owner/repo 導出規則を
//! 含まない）で起動し、整形・repo 導出・changelog/homepage 抽出を **Rust 側**で行って [`NixPackage`] へ畳む。
//!
//! repo（owner/repo）導出の優先: ①`meta.homepage` が github ②無ければ `src`（owner+repo 直接、無ければ
//! url/urls の github URL）③無ければ `meta.changelog` の github URL。github 由来が取れなければ空（version-only
//! 行き）。すべて信頼境界外の値であり、実取得時に `host_of` の構造的検査（https 限定・credential/IP リテラル/
//! localhost/単一ラベル/内部 DNS 拒否）で機械検証する。
//!
//! flake.lock の rev 抽出（[`lock_node_rev`]）も serde_json で行う。

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
const RAW_EVAL_APPLY: &str = r#"ps: builtins.listToAttrs (map (p:
  let
    meta = p.meta or {};
    src = p.src or {};
  in {
  name = p.pname or (builtins.parseDrvName (p.name or "")).name;
  value = {
    version = p.version or "";
    homepage = meta.homepage or null;
    changelog = meta.changelog or null;
    src_owner = src.owner or null;
    src_repo = src.repo or null;
    src_url = src.url or null;
    src_urls = src.urls or null;
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
    let home = eval_package_list(&home_manager_packages_attr_path(reference, user))?;
    let system = eval_package_list(&format!(".#{reference}.config.environment.systemPackages"))?;
    // home を先に、同名は system 側で上書きする（後勝ち）。`chain` で system を後に置き collect する。
    Ok(home.into_iter().chain(system).collect())
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
        .map(|(name, package)| {
            let derived = derive_package(&name, package);
            (name, derived)
        })
        .collect())
}

/// Home Manager の宣言パッケージ attr path を、ユーザー名を Nix 文字列キーとして組み立てる。
fn home_manager_packages_attr_path(reference: &str, user: &str) -> String {
    format!(
        ".#{reference}.config.home-manager.users.\"{}\".home.packages",
        escape_nix_string(user)
    )
}

/// CLI/eval 由来の値を Nix の二重引用符文字列キーへ安全に埋め込む。
fn escape_nix_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("${", "\\${")
}

/// 生評価値 1 件を導出済み [`NixPackage`]（version + repo/changelog/homepage）へ翻訳する純粋関数。
fn derive_package(name: &str, raw: RawPackage) -> NixPackage {
    let homepage = as_str(&raw.homepage);
    let changelog = package_notes_source(name, &raw.version, &homepage, &as_str(&raw.changelog));
    NixPackage {
        version: raw.version,
        repo: derive_repo(
            name,
            &homepage,
            &as_str(&raw.src_owner),
            &as_str(&raw.src_repo),
            &as_str(&raw.src_url),
            first_url(&raw.src_urls),
            &changelog,
        ),
        // changelog を Releases API 空振り時の raw フォールバック取得元にする。changelog が無いときは空のまま
        // にし、homepage の HTML を生ノート seed に固定しない（homepage は AI の fetch_url 探索ヒントに残る）。
        notes_source: changelog,
        homepage,
    }
}

/// owner/repo を homepage(github) → src → changelog(github) → package 固有 fallback の優先で導出する純粋関数
/// （取れなければ空文字）。
fn derive_repo(
    name: &str,
    homepage: &str,
    src_owner: &str,
    src_repo: &str,
    src_url: &str,
    src_first_url: Option<&str>,
    changelog: &str,
) -> String {
    if let Some(repo) = super::wire::repo_from_github_url(homepage) {
        return repo;
    }
    if !src_owner.is_empty() && !src_repo.is_empty() {
        return format!("{src_owner}/{src_repo}");
    }
    if let Some(repo) = super::wire::repo_from_github_url(src_url) {
        return repo;
    }
    if let Some(repo) = src_first_url.and_then(super::wire::repo_from_github_url) {
        return repo;
    }
    super::wire::repo_from_github_url(changelog)
        .unwrap_or_else(|| repo_hint_for_package(name, homepage))
}

fn repo_hint_for_package(name: &str, homepage: &str) -> String {
    match (name, homepage) {
        ("nix", "https://nixos.org/nix") => "NixOS/nix".to_string(),
        _ => String::new(),
    }
}

fn nix_release_notes_url(version: &str) -> Option<String> {
    let version = version.trim().trim_start_matches('v');
    let mut parts = version.split('.');
    let major = non_empty(parts.next())?;
    let minor = non_empty(parts.next())?;
    Some(format!(
        "https://nix.dev/manual/nix/{major}.{minor}/release-notes/rl-{major}.{minor}"
    ))
}

fn rust_release_notes_url(version: &str) -> Option<String> {
    let version = version.trim();
    if version.is_empty() {
        return None;
    }
    Some("https://doc.rust-lang.org/stable/releases.html".to_string())
}

fn github_release_tag_url(repo: &str, version: &str) -> Option<String> {
    let version = version.trim();
    if repo.is_empty() || version.is_empty() {
        return None;
    }
    let tag = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    };
    Some(format!("https://github.com/{repo}/releases/tag/{tag}"))
}

fn chrome_releases_search_url(version: &str) -> Option<String> {
    let version = version.trim();
    if version.is_empty() {
        return None;
    }
    Some(format!(
        "https://chromereleases.googleblog.com/search?q={version}"
    ))
}

fn package_notes_source(name: &str, version: &str, homepage: &str, changelog: &str) -> String {
    if !changelog.is_empty() {
        return changelog.to_string();
    }
    match (name, homepage) {
        ("coreutils", "https://www.gnu.org/software/coreutils/") => {
            "https://cgit.git.savannah.gnu.org/cgit/coreutils.git/plain/NEWS".to_string()
        }
        ("nix", "https://nixos.org/nix") => nix_release_notes_url(version).unwrap_or_default(),
        ("google-chrome", "https://www.google.com/chrome/browser/") => {
            "https://chromereleases.googleblog.com/".to_string()
        }
        ("chromedriver", "https://chromedriver.chromium.org/") => {
            chrome_releases_search_url(version).unwrap_or_default()
        }
        ("docker-compose", _) => {
            github_release_tag_url("docker/compose", version).unwrap_or_default()
        }
        ("rustfmt", _) => rust_release_notes_url(version).unwrap_or_default(),
        ("discord", "https://discordapp.com/") => {
            "https://discord.com/tags/patch-notes".to_string()
        }
        ("slack", "https://slack.com/intl/en-jp/downloads/mac")
        | ("slack", "https://slack.com") => "https://slack.com/release-notes/mac".to_string(),
        ("temurin-bin", "https://adoptium.net/") => {
            "https://adoptium.net/temurin/release-notes".to_string()
        }
        _ => String::new(),
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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

/// flake.lock の `nodes.<node>.locked.rev` を serde_json で取り出す（workflow が rev 抽出に呼ぶ）。
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
    //! （list/非文字列→空）・Home Manager attr path の文字列キー化と escape・flake.lock rev 抽出を
    //! network/nix 抜きで固定する。

    use super::*;

    #[test]
    fn home_manager_packages_attr_path_quotes_user_as_string_key() {
        assert_eq!(
            home_manager_packages_attr_path("darwinConfigurations.mac", "user-name"),
            r#".#darwinConfigurations.mac.config.home-manager.users."user-name".home.packages"#
        );
    }

    #[test]
    fn home_manager_packages_attr_path_escapes_nix_string_key() {
        assert_eq!(
            home_manager_packages_attr_path("darwinConfigurations.mac", r#"a\b"${bad}"#),
            r#".#darwinConfigurations.mac.config.home-manager.users."a\\b\"\${bad}".home.packages"#
        );
    }

    #[test]
    fn derive_repo_prefers_homepage_then_src_then_changelog() {
        // ① homepage が github → そこから（src/changelog より優先）。
        assert_eq!(
            derive_repo(
                "ignored",
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
                "ignored",
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
            derive_repo("ignored", "", "", "", "https://github.com/o/r", None, ""),
            "o/r"
        );
        // ②'' src.url 無し → src.urls 先頭の github。
        assert_eq!(
            derive_repo(
                "ignored",
                "",
                "",
                "",
                "",
                Some("https://github.com/o/r2"),
                ""
            ),
            "o/r2"
        );
        // ③ homepage/src 無し → changelog の github、末尾 `.git` 剥がし。
        assert_eq!(
            derive_repo(
                "ignored",
                "",
                "",
                "",
                "",
                None,
                "https://github.com/owner/proj.git",
            ),
            "owner/proj"
        );
        // ④ いずれも非 github → 空文字。
        assert_eq!(
            derive_repo(
                "ignored",
                "https://gitlab.com/o/r",
                "",
                "",
                "",
                None,
                "https://example.com/changelog",
            ),
            ""
        );
        // ⑤ package 固有 fallback。
        assert_eq!(
            derive_repo("nix", "https://nixos.org/nix", "", "", "", None, ""),
            "NixOS/nix"
        );
    }

    #[test]
    fn releases_url_from_github_url_normalizes_release_variants_only() {
        assert_eq!(
            crate::update_history::wire::releases_url_from_github_url(
                "https://github.com/o/r/releases"
            )
            .as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            crate::update_history::wire::releases_url_from_github_url(
                "https://github.com/o/r/releases/tag/v1.2.3"
            )
            .as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            crate::update_history::wire::releases_url_from_github_url(
                "https://github.com/o/r/releases/download/v1/x.zip"
            )
            .as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            crate::update_history::wire::releases_url_from_github_url("https://github.com/o/r"),
            None
        );
        assert_eq!(
            crate::update_history::wire::releases_url_from_github_url(
                "https://github.com/o/r/issues/1"
            ),
            None
        );
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
    fn derive_package_keeps_notes_source_empty_without_changelog() {
        let raw = RawPackage {
            version: "1.2.3".to_string(),
            homepage: serde_json::json!("https://homepage.example/"),
            changelog: serde_json::Value::Null,
            src_owner: serde_json::Value::Null,
            src_repo: serde_json::Value::Null,
            src_url: serde_json::Value::Null,
            src_urls: serde_json::Value::Null,
        };
        let package = derive_package("ignored", raw);
        assert_eq!(package.version, "1.2.3");
        // changelog 無し → notes_source は空のまま（homepage の HTML を生ノート seed に固定しない）。
        assert_eq!(package.notes_source, "");
        // homepage は AI の fetch_url 探索ヒントとして保持する。
        assert_eq!(package.homepage, "https://homepage.example/");
    }

    #[test]
    fn derive_package_keeps_repo_from_github_homepage_without_changelog() {
        // 退行固定: homepage が github の bare repo root（`github.com/<owner>/<repo>`）を指し changelog が無い
        // パッケージ（uv/zed-editor/tree 等）は、homepage HTML を notes seed に固定しない一方で owner/repo は
        // homepage から導出し続ける。repo が非空である限り下流は mechanical(releases-range)/ai(fetch_url) に乗せ、
        // route=none（version-only）へは落ちない。
        let raw = RawPackage {
            version: "0.7.0".to_string(),
            homepage: serde_json::json!("https://github.com/astral-sh/uv"),
            changelog: serde_json::Value::Null,
            src_owner: serde_json::Value::Null,
            src_repo: serde_json::Value::Null,
            src_url: serde_json::Value::Null,
            src_urls: serde_json::Value::Null,
        };
        let package = derive_package("ignored", raw);
        // homepage HTML を notes seed にしない一方で owner/repo は homepage(github) から導出する。
        assert_eq!(package.repo, "astral-sh/uv");
        // changelog 不在でも homepage HTML を機械 seed に固定しない（notes_source は空）。
        assert_eq!(package.notes_source, "");
        // homepage は AI fetch_url 探索ヒントとして保持する。
        assert_eq!(package.homepage, "https://github.com/astral-sh/uv");
    }

    #[test]
    fn derive_package_uses_changelog_as_notes_source() {
        let raw = RawPackage {
            version: "1.2.3".to_string(),
            homepage: serde_json::json!("https://homepage.example/"),
            changelog: serde_json::json!("https://github.com/o/r/blob/main/CHANGELOG.md"),
            src_owner: serde_json::Value::Null,
            src_repo: serde_json::Value::Null,
            src_url: serde_json::Value::Null,
            src_urls: serde_json::Value::Null,
        };
        let package = derive_package("ignored", raw);
        // changelog あり → notes_source は changelog（homepage は別途ヒントに残る）。
        assert_eq!(
            package.notes_source,
            "https://github.com/o/r/blob/main/CHANGELOG.md"
        );
        assert_eq!(package.homepage, "https://homepage.example/");
    }

    #[test]
    fn derive_package_adds_repo_fallback_for_nix_homepage() {
        let raw = RawPackage {
            version: "2.34.7+1".to_string(),
            homepage: serde_json::json!("https://nixos.org/nix"),
            changelog: serde_json::Value::Null,
            src_owner: serde_json::Value::Null,
            src_repo: serde_json::Value::Null,
            src_url: serde_json::Value::Null,
            src_urls: serde_json::Value::Null,
        };
        let package = derive_package("nix", raw);
        assert_eq!(package.repo, "NixOS/nix");
        assert_eq!(
            package.notes_source,
            "https://nix.dev/manual/nix/2.34/release-notes/rl-2.34"
        );
        assert_eq!(package.homepage, "https://nixos.org/nix");
    }

    #[test]
    fn derive_package_adds_notes_source_fallbacks_for_chrome_family() {
        let google_chrome = derive_package(
            "google-chrome",
            RawPackage {
                version: "149.0.7827.115".to_string(),
                homepage: serde_json::json!("https://www.google.com/chrome/browser/"),
                changelog: serde_json::Value::Null,
                src_owner: serde_json::Value::Null,
                src_repo: serde_json::Value::Null,
                src_url: serde_json::Value::Null,
                src_urls: serde_json::Value::Null,
            },
        );
        assert_eq!(
            google_chrome.notes_source,
            "https://chromereleases.googleblog.com/"
        );

        let chromedriver = derive_package(
            "chromedriver",
            RawPackage {
                version: "149.0.7827.103".to_string(),
                homepage: serde_json::json!("https://chromedriver.chromium.org/"),
                changelog: serde_json::Value::Null,
                src_owner: serde_json::Value::Null,
                src_repo: serde_json::Value::Null,
                src_url: serde_json::Value::Null,
                src_urls: serde_json::Value::Null,
            },
        );
        assert_eq!(
            chromedriver.notes_source,
            "https://chromereleases.googleblog.com/search?q=149.0.7827.103"
        );
    }

    #[test]
    fn derive_package_adds_notes_source_fallbacks_for_desktop_apps() {
        let coreutils = derive_package(
            "coreutils",
            RawPackage {
                version: "9.11".to_string(),
                homepage: serde_json::json!("https://www.gnu.org/software/coreutils/"),
                changelog: serde_json::Value::Null,
                src_owner: serde_json::Value::Null,
                src_repo: serde_json::Value::Null,
                src_url: serde_json::Value::Null,
                src_urls: serde_json::Value::Null,
            },
        );
        assert_eq!(
            coreutils.notes_source,
            "https://cgit.git.savannah.gnu.org/cgit/coreutils.git/plain/NEWS"
        );

        let discord = derive_package(
            "discord",
            RawPackage {
                version: "0.0.393".to_string(),
                homepage: serde_json::json!("https://discordapp.com/"),
                changelog: serde_json::Value::Null,
                src_owner: serde_json::Value::Null,
                src_repo: serde_json::Value::Null,
                src_url: serde_json::Value::Null,
                src_urls: serde_json::Value::Null,
            },
        );
        assert_eq!(discord.notes_source, "https://discord.com/tags/patch-notes");

        let slack = derive_package(
            "slack",
            RawPackage {
                version: "4.49.89".to_string(),
                homepage: serde_json::json!("https://slack.com"),
                changelog: serde_json::Value::Null,
                src_owner: serde_json::Value::Null,
                src_repo: serde_json::Value::Null,
                src_url: serde_json::Value::Null,
                src_urls: serde_json::Value::Null,
            },
        );
        assert_eq!(slack.notes_source, "https://slack.com/release-notes/mac");

        let docker_compose = derive_package(
            "docker-compose",
            RawPackage {
                version: "5.1.4".to_string(),
                homepage: serde_json::json!("https://github.com/docker/compose"),
                changelog: serde_json::Value::Null,
                src_owner: serde_json::Value::Null,
                src_repo: serde_json::Value::Null,
                src_url: serde_json::Value::Null,
                src_urls: serde_json::Value::Null,
            },
        );
        assert_eq!(
            docker_compose.notes_source,
            "https://github.com/docker/compose/releases/tag/v5.1.4"
        );

        let rustfmt = derive_package(
            "rustfmt",
            RawPackage {
                version: "1.95.0".to_string(),
                homepage: serde_json::json!("https://github.com/rust-lang-nursery/rustfmt"),
                changelog: serde_json::Value::Null,
                src_owner: serde_json::Value::Null,
                src_repo: serde_json::Value::Null,
                src_url: serde_json::Value::Null,
                src_urls: serde_json::Value::Null,
            },
        );
        assert_eq!(
            rustfmt.notes_source,
            "https://doc.rust-lang.org/stable/releases.html"
        );

        let temurin = derive_package(
            "temurin-bin",
            RawPackage {
                version: "21.0.11".to_string(),
                homepage: serde_json::json!("https://adoptium.net/"),
                changelog: serde_json::Value::Null,
                src_owner: serde_json::Value::Null,
                src_repo: serde_json::Value::Null,
                src_url: serde_json::Value::Null,
                src_urls: serde_json::Value::Null,
            },
        );
        assert_eq!(
            temurin.notes_source,
            "https://adoptium.net/temurin/release-notes"
        );
    }

    #[test]
    fn lock_node_rev_extracts_locked_rev() -> Result<()> {
        let path =
            std::env::temp_dir().join(format!("dotfiles-uh-lock-{}.json", std::process::id()));
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

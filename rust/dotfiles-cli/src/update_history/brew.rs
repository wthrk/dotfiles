//! 宣言 cask の版差分を old/new tap rev の cask `.rb` 定義から決定論的に算出する（ライブ brew 非問い合わせ）。
//!
//! reqwest で `raw.githubusercontent.com/homebrew/homebrew-cask/<rev>/Casks/<subdir>/<name>.rb` を取得して
//! `version "..."` を解析する。全 cask（`auto_updates true` = bitwarden/codex-app/ghostty も含む）が `homebrew.nix`
//! の `greedyCasks = true` で無人 upgrade の対象になるため、版差分の追跡対象にする。subdir は cask 名の先頭文字
//! （font cask は `font/font-<X>`、`<X>` は `font-` の次の 1 文字）。
//!
//! greedy 有効化の前提は「全 cask が sha256 固定」。new rev の cask `.rb` に `sha256 :no_check`（未固定成果物）が
//! あれば、無人 upgrade が外部成果物を再現性なく差し替えうるため fail-closed にする（[`assert_pinned`]）。
//!
//! cask 一覧は `nix/modules/homebrew.nix` の `casks = [ ... ]` から抽出する（switch が導入する cask と同一源）。

use anyhow::bail;

use super::diff::{DeltaSource, VersionDelta, version_ordering};
use super::notes::safe_https_fetch;
use super::wire::{ChangeKind, is_allowed_url};
use crate::Result;

/// 宣言 cask の old→new tap rev 版差分を算出する。
///
/// `casks_nix` は `nix/modules/homebrew.nix` のテキスト（`casks = [ ... ]` を抽出）。各 cask の `version` を
/// 両 rev の cask `.rb` から取り、版変化のあるものだけ [`VersionDelta`] にする。両 rev とも取得不能 / 版変更なし
/// は捨てる（ノイズ抑制）。`greedyCasks = true` で全 cask が無人 upgrade 対象になるため `auto_updates true` の
/// cask も追跡する。new rev の `.rb` が `sha256 :no_check` の未固定成果物なら fail-closed にする（[`assert_pinned`]）。
/// `fetch` は cask `.rb` 取得 seam（本番は reqwest、テストは fake）。
pub(crate) fn diff_casks(
    casks_nix: &str,
    rev_old: &str,
    rev_new: &str,
    fetch: &dyn Fn(&str) -> Result<Option<String>>,
) -> Result<Vec<VersionDelta>> {
    // 各 cask の old/new 版差分を `Result<Option<delta>>` へ翻訳して、Err 伝播（`collect::<Result<_>>`）と
    // 版変化なし（`None`）の除去（`flatten`）を分けて行う。
    parse_cask_list(casks_nix)
        .into_iter()
        .map(|name| cask_delta(rev_old, rev_new, name, fetch))
        .collect::<Result<Vec<Option<VersionDelta>>>>()
        .map(|deltas| deltas.into_iter().flatten().collect())
}

/// 1 cask の old/new 版を取得し、版変化があれば [`VersionDelta`] を組む（取得不能/版不変は `None`）。
///
/// new rev の `.rb` 本文に対しては成果物固定を [`assert_pinned`] で検査し、`sha256 :no_check` なら fail-closed。
fn cask_delta(
    rev_old: &str,
    rev_new: &str,
    name: String,
    fetch: &dyn Fn(&str) -> Result<Option<String>>,
) -> Result<Option<VersionDelta>> {
    let old = cask_version(rev_old, &name, fetch)?;
    let new = cask_version_pinned(rev_new, &name, fetch)?;
    let change = match (&old, &new) {
        (None, None) => return Ok(None),
        (None, Some(_)) => ChangeKind::Added,
        (Some(_), None) => ChangeKind::Removed,
        (Some(old_v), Some(new_v)) => match version_ordering(old_v, new_v) {
            std::cmp::Ordering::Equal => return Ok(None),
            std::cmp::Ordering::Less => ChangeKind::Upgraded,
            std::cmp::Ordering::Greater => ChangeKind::Downgraded,
        },
    };
    Ok(Some(VersionDelta {
        name,
        old,
        new,
        change,
        source: DeltaSource::BrewTap,
        repo: None,
        notes_source: None,
        homepage: None,
    }))
}

/// 本番経路: reqwest で cask `.rb` を取得する fetch seam（redirect 不追従・https 限定・有界本文）。
pub(crate) fn fetch_cask_rb(url: &str) -> Result<Option<String>> {
    if !is_allowed_url(url) {
        return Ok(None);
    }
    safe_https_fetch(url)
}

/// 単一 cask の `version "..."` を tap rev の `.rb` から取得する（取得不能 / 未定義は `None`）。
fn cask_version(
    rev: &str,
    name: &str,
    fetch: &dyn Fn(&str) -> Result<Option<String>>,
) -> Result<Option<String>> {
    let url = cask_rb_url(rev, name);
    Ok(fetch(&url)?.as_deref().and_then(parse_cask_version))
}

/// new rev 用: `.rb` を取得し成果物固定を [`assert_pinned`] で検査してから `version "..."` を取り出す。
///
/// greedy 有効化（`homebrew.nix` の `greedyCasks = true`）で全 cask が無人 upgrade 対象になるため、未固定成果物
/// （`sha256 :no_check`）が new rev に現れたら fail-closed にし、外部成果物の再現性なき無人差し替えを阻む。
/// `.rb` 取得不能（`None`）は検査対象が無いとみなして通す（版も `None` を返す）。
fn cask_version_pinned(
    rev: &str,
    name: &str,
    fetch: &dyn Fn(&str) -> Result<Option<String>>,
) -> Result<Option<String>> {
    let url = cask_rb_url(rev, name);
    match fetch(&url)? {
        None => Ok(None),
        Some(rb) => assert_pinned(name, &rb).map(|()| parse_cask_version(&rb)),
    }
}

/// cask `.rb` 本文が成果物を固定しているかを検査し、`sha256 :no_check` なら fail-closed にする。
///
/// greedy 有効下では未固定成果物が無人差し替えされうるため、`greedyCasks = true` の前提「全 cask が sha256 固定」
/// を守れない cask が tap rev に現れた時点で停止し、原因 cask 名を添える。
fn assert_pinned(name: &str, rb: &str) -> Result<()> {
    if has_no_check_sha256(rb) {
        bail!(
            "cask `{name}` は `sha256 :no_check`（未固定成果物）。greedy 有効化の前提（全 cask sha256 固定）に\
             反するため停止する。`homebrew.nix` から外すか sha256 を固定すること"
        );
    }
    Ok(())
}

/// cask `.rb` 本文に `sha256 :no_check`（成果物を固定しない指定）があるかを判定する純粋関数。
///
/// 行頭の空白を除いた `sha256` 宣言行で、続く非空白トークンが `:no_check` のものを未固定とみなす。
fn has_no_check_sha256(rb: &str) -> bool {
    rb.lines().any(|line| {
        line.trim_start()
            .strip_prefix("sha256")
            .map(str::trim_start)
            .is_some_and(|rest| rest.starts_with(":no_check"))
    })
}

/// `raw.githubusercontent.com` の cask `.rb` 取得 URL を構築する純粋関数。font cask は `font/font-<X>`
/// （`<X>` は `font-` の次の 1 文字を小文字化）、非 font cask は cask 名の先頭文字を subdir にする。
fn cask_rb_url(rev: &str, name: &str) -> String {
    let subdir = match name
        .strip_prefix("font-")
        .and_then(|rest| rest.chars().next())
    {
        Some(c) => format!("font/font-{}", c.to_ascii_lowercase()),
        None => name
            .chars()
            .next()
            .map(|c| c.to_ascii_lowercase().to_string())
            .unwrap_or_default(),
    };
    format!(
        "https://raw.githubusercontent.com/homebrew/homebrew-cask/{rev}/Casks/{subdir}/{name}.rb"
    )
}

/// cask `.rb` 本文から最初の `version "..."` を取り出す純粋関数（`version :latest` 等は数十行ルールで非対象）。
fn parse_cask_version(rb: &str) -> Option<String> {
    for line in rb.lines() {
        let trimmed = line.trim_start();
        let Some(after) = trimmed.strip_prefix("version") else {
            continue;
        };
        if !after.starts_with(char::is_whitespace) {
            continue;
        }
        let Some(open) = after.find('"') else {
            continue;
        };
        let rest = &after[open + 1..];
        let Some(close) = rest.find('"') else {
            continue;
        };
        let value = &rest[..close];
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// `nix/modules/homebrew.nix` の `casks = [ "a" "b" ... ];` から cask 名を抽出する純粋関数。
fn parse_cask_list(nix: &str) -> Vec<String> {
    let Some(after) = nix.split_once("casks = [").map(|(_, rest)| rest) else {
        return Vec::new();
    };
    let block = after
        .split_once(']')
        .map(|(block, _)| block)
        .unwrap_or(after);
    block
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    //! cask list 抽出・cask URL 構築（letter/font subdir）・version 解析・版差分（auto_updates も追跡・
    //! added/removed/upgraded・ノイズ除去）・`sha256 :no_check` の fail-closed を network 抜きで固定する。

    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parse_cask_list_extracts_quoted_names() {
        let nix = "  casks = [\n    \"azookey\"\n    \"bitwarden\"\n    \"font-cica\"\n  ];\n";
        assert_eq!(parse_cask_list(nix), ["azookey", "bitwarden", "font-cica"]);
        assert!(parse_cask_list("no casks here").is_empty());
    }

    #[test]
    fn cask_rb_url_resolves_letter_and_font_subdir() {
        assert_eq!(
            cask_rb_url("deadbeef", "firefox"),
            "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/f/firefox.rb"
        );
        assert_eq!(
            cask_rb_url("deadbeef", "Discord"),
            "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/d/Discord.rb"
        );
        assert_eq!(
            cask_rb_url("deadbeef", "font-cica"),
            "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/font/font-c/font-cica.rb"
        );
    }

    #[test]
    fn parse_cask_version_reads_first_version_string() {
        assert_eq!(
            parse_cask_version("cask \"x\" do\n  version \"1.2.3\"\n  sha256 \"...\"\nend\n")
                .as_deref(),
            Some("1.2.3")
        );
        // `version :latest`（クォート無し）は非対象。
        assert_eq!(
            parse_cask_version("cask \"x\" do\n  version :latest\nend\n"),
            None
        );
        assert_eq!(parse_cask_version("no version line"), None);
    }

    #[test]
    fn diff_casks_tracks_auto_updates_and_computes_changes() -> Result<()> {
        // azookey: upgrade、yubico-authenticator: 版不変→除外、bitwarden（auto_updates 相当）: upgrade で追跡。
        let nix = "casks = [ \"azookey\" \"bitwarden\" \"yubico-authenticator\" ];";
        let old: BTreeMap<String, String> = [
            (cask_rb_url("old", "azookey"), version_rb("1.0")),
            (cask_rb_url("old", "bitwarden"), version_rb("2024.1")),
            (
                cask_rb_url("old", "yubico-authenticator"),
                version_rb("2.0"),
            ),
        ]
        .into_iter()
        .collect();
        let new: BTreeMap<String, String> = [
            (cask_rb_url("new", "azookey"), version_rb("1.1")),
            (cask_rb_url("new", "bitwarden"), version_rb("2024.2")),
            (
                cask_rb_url("new", "yubico-authenticator"),
                version_rb("2.0"),
            ),
        ]
        .into_iter()
        .collect();
        let fetch = |url: &str| -> Result<Option<String>> {
            Ok(old.get(url).or_else(|| new.get(url)).cloned())
        };
        let deltas = diff_casks(nix, "old", "new", &fetch)?;
        // azookey と bitwarden（auto_updates）の双方が追跡される。yubico-authenticator は版不変で除外。
        assert_eq!(deltas.len(), 2);
        let azookey = deltas
            .iter()
            .find(|d| d.name == "azookey")
            .ok_or_else(|| anyhow::anyhow!("azookey delta missing"))?;
        assert_eq!(azookey.change, ChangeKind::Upgraded);
        assert_eq!(azookey.old.as_deref(), Some("1.0"));
        assert_eq!(azookey.new.as_deref(), Some("1.1"));
        assert_eq!(azookey.source, DeltaSource::BrewTap);
        let bitwarden = deltas
            .iter()
            .find(|d| d.name == "bitwarden")
            .ok_or_else(|| anyhow::anyhow!("bitwarden delta missing"))?;
        assert_eq!(bitwarden.change, ChangeKind::Upgraded);
        assert_eq!(bitwarden.old.as_deref(), Some("2024.1"));
        assert_eq!(bitwarden.new.as_deref(), Some("2024.2"));
        Ok(())
    }

    #[test]
    fn diff_casks_fails_closed_on_no_check_sha256() {
        // new rev の `.rb` が `sha256 :no_check`（未固定成果物）なら greedy 前提に反するため停止する。
        let nix = "casks = [ \"loose\" ];";
        let fetch = |url: &str| -> Result<Option<String>> {
            Ok(if url == cask_rb_url("new", "loose") {
                Some("cask \"loose\" do\n  version \"1.0\"\n  sha256 :no_check\nend\n".to_string())
            } else {
                None
            })
        };
        let error = diff_casks(nix, "old", "new", &fetch)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("loose"),
            "error should name the cask: {error}"
        );
        assert!(
            error.contains("no_check"),
            "error should cite no_check: {error}"
        );
    }

    #[test]
    fn has_no_check_sha256_distinguishes_pinned_from_unpinned() {
        assert!(has_no_check_sha256("  sha256 :no_check\n"));
        assert!(!has_no_check_sha256("  sha256 \"abc123\"\n"));
        assert!(!has_no_check_sha256("  version \"1.0\"\n"));
    }

    #[test]
    fn diff_casks_marks_added_and_removed() -> Result<()> {
        let nix = "casks = [ \"newcask\" \"oldcask\" ];";
        let fetch = |url: &str| -> Result<Option<String>> {
            Ok(if url == cask_rb_url("new", "newcask") {
                Some(version_rb("3.0"))
            } else if url == cask_rb_url("old", "oldcask") {
                Some(version_rb("1.0"))
            } else {
                None
            })
        };
        let deltas = diff_casks(nix, "old", "new", &fetch)?;
        let added = deltas.iter().find(|d| d.name == "newcask");
        let removed = deltas.iter().find(|d| d.name == "oldcask");
        assert_eq!(added.map(|d| d.change), Some(ChangeKind::Added));
        assert_eq!(removed.map(|d| d.change), Some(ChangeKind::Removed));
        Ok(())
    }

    fn version_rb(version: &str) -> String {
        format!("cask \"x\" do\n  version \"{version}\"\nend\n")
    }
}

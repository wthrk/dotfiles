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

use anyhow::{Context, bail};

use super::diff::{DeltaSource, VersionDelta, version_ordering};
use super::notes::safe_https_fetch;
use super::wire::{ChangeKind, is_allowed_url};
use crate::Result;

/// 宣言 cask の old→new tap rev 版差分を算出する。
///
/// `casks_nix` は `nix/modules/homebrew.nix` のテキスト（`casks = [ ... ]` を抽出）。各 cask の `version` を
/// 両 rev の cask `.rb` から取り、版変化のあるものだけ [`VersionDelta`] にする。版変更なしは捨てる（ノイズ抑制）。
/// old/new の本文取得、または `version "..."` の解析に失敗した場合は差分や resource state へ縮退せず停止する。
/// `greedyCasks = true` で全 cask が無人 upgrade 対象になるため `auto_updates true` の cask も追跡する。new rev の
/// `.rb` が `sha256 :no_check` の未固定成果物なら fail-closed（`Err`）にする（[`assert_pinned`]）。
/// `fetch` は cask `.rb` 本文を返す seam（本番は reqwest、テストは fake）であり、HTTP status を Cask の有無へ
/// 翻訳しない。
pub(crate) fn diff_casks(
    casks_nix: &str,
    rev_old: &str,
    rev_new: &str,
    fetch: &dyn Fn(&str) -> Result<String>,
) -> Result<Vec<VersionDelta>> {
    // 各 cask の old/new 版差分を `Result<Option<delta>>` へ翻訳して、Err 伝播（`collect::<Result<_>>`）と
    // 版変化なし（`None`）の除去（`flatten`）を分けて行う。
    parse_cask_list(casks_nix)?
        .into_iter()
        .map(|name| cask_delta(rev_old, rev_new, name, fetch))
        .collect::<Result<Vec<Option<VersionDelta>>>>()
        .map(|deltas| deltas.into_iter().flatten().collect())
}

/// 1 cask の old/new 版を取得し、版変化があれば [`VersionDelta`] を組む（版不変は `None`）。
///
/// old/new の取得または version 解析に失敗した時点で、差分なし・追加・削除・取得不能のいずれにも翻訳せず `Err`
/// を伝播する。new rev の `.rb` 本文に対しては成果物固定を [`assert_pinned`] で検査し、`sha256 :no_check` なら
/// fail-closed。
fn cask_delta(
    rev_old: &str,
    rev_new: &str,
    name: String,
    fetch: &dyn Fn(&str) -> Result<String>,
) -> Result<Option<VersionDelta>> {
    let new = cask_version_pinned(rev_new, &name, fetch)?;
    let old = cask_version(rev_old, &name, fetch)?;
    let change = match version_ordering(&old, &new) {
        std::cmp::Ordering::Equal => return Ok(None),
        std::cmp::Ordering::Less => ChangeKind::Upgraded,
        std::cmp::Ordering::Greater => ChangeKind::Downgraded,
    };
    Ok(Some(VersionDelta {
        name,
        old: Some(old),
        new: Some(new),
        change,
        source: DeltaSource::BrewTap,
        repo: None,
        notes_source: None,
        homepage: None,
    }))
}

/// 本番経路: reqwest で cask `.rb` の成功応答の非空本文を取得する fetch seam。
///
/// 許可外 URL は Cask 不在へ偽装せず拒否する。取得自体は [`safe_https_fetch`]（redirect 不追従・https 限定・
/// 有界本文）へ委譲し、非成功 HTTP status、transport error、空本文は Cask resource state へ意味付けせず伝播する。
///
/// Evidence: `reqwest::blocking::Response::status` と `http::StatusCode::is_success` は response status と 2xx
/// 判定だけを定義する。GitHub REST は 404 を認証されていない private resource にも返すため、HTTP 404 だけを
/// Cask 不在に翻訳しない。
/// - <https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.Response.html#method.status>
/// - <https://docs.rs/http/1.4.1/http/status/struct.StatusCode.html#method.is_success>
/// - <https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api?apiVersion=2022-11-28#404-not-found-for-an-existing-resource>
pub(crate) fn fetch_cask_rb(url: &str) -> Result<String> {
    if !is_allowed_url(url) {
        bail!("refusing structurally disallowed cask URL `{url}`");
    }
    safe_https_fetch(url)?.context("cask `.rb` response body is empty")
}

/// 単一 cask の tap rev から quoted `version` を取得する。
///
/// `version :latest` や構文不正を Cask 不在、削除、または version 差分なしとして扱わない。現在の更新履歴 schema
/// は quoted version の old/new を記録するため、この取得不能は停止する。
fn cask_version(rev: &str, name: &str, fetch: &dyn Fn(&str) -> Result<String>) -> Result<String> {
    let url = cask_rb_url(rev, name);
    let rb =
        fetch(&url).with_context(|| format!("failed to fetch cask `{name}` at rev `{rev}`"))?;
    parse_cask_version(&rb).with_context(|| {
        format!("cask `{name}` at rev `{rev}` has no quoted `version \"...\"` declaration")
    })
}

/// new rev 用: `.rb` を取得し成果物固定を [`assert_pinned`] で検査してから quoted version を取得する。
///
/// greedy 有効化（`homebrew.nix` の `greedyCasks = true`）で全 cask が無人 upgrade 対象になるため、未固定成果物
/// （`sha256 :no_check`）が new rev に現れたら fail-closed にし、外部成果物の再現性なき無人差し替えを阻む。
fn cask_version_pinned(
    rev: &str,
    name: &str,
    fetch: &dyn Fn(&str) -> Result<String>,
) -> Result<String> {
    let url = cask_rb_url(rev, name);
    let rb =
        fetch(&url).with_context(|| format!("failed to fetch cask `{name}` at new rev `{rev}`"))?;
    assert_pinned(name, &rb)?;
    parse_cask_version(&rb).with_context(|| {
        format!("cask `{name}` at new rev `{rev}` has no quoted `version \"...\"` declaration")
    })
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

/// cask `.rb` 本文に `:no_check`（成果物を固定しない指定）の `sha256` スタンザがあるかを判定する純粋関数。
///
/// `sha256` スタンザは同一行直後（`sha256 :no_check`）だけでなく、arch 別指定（`sha256 arm: :no_check, intel:
/// "..."`）や継続行（`arm: :no_check` / `intel: :no_check`）でも未固定を宣言しうる。`sha256` 宣言行から、続く
/// 継続行（行頭が arch ラベル `arm:` / `intel:` で始まるか、行末が `,` で続く）までを 1 スタンザとして集め、その
/// 範囲に `:no_check` トークンが現れれば未固定とみなす。誤検出を避けるため `:no_check` は語境界（直後が識別子
/// 文字でない）でだけ拾い、`version "..."` 等の文字列リテラル内の偶発一致は対象 sha256 スタンザ外なので拾わない。
fn has_no_check_sha256(rb: &str) -> bool {
    let lines: Vec<&str> = rb.lines().collect();
    lines.iter().enumerate().any(|(index, line)| {
        line.trim_start()
            .strip_prefix("sha256")
            .is_some_and(|rest| {
                // `sha256abc` のような別トークンを誤って拾わない（`sha256` 直後は空白か行末）。
                (rest.is_empty() || rest.starts_with(char::is_whitespace))
                    && sha256_stanza_lines(&lines, index, rest).any(stanza_has_no_check)
            })
    })
}

/// `sha256` 宣言行とそれに連なる継続行群を、スタンザ範囲のトークン文字列の列として返す純粋関数。
///
/// 宣言行は `sha256` を除いた残り（`rest`）。継続行は、宣言行が `,` で終わるか、行頭が arch ラベル
/// （`arm:` / `intel:` / `x86_64:` 等の `<ident>:`）で始まる限り、`sha256` 行の直後から順に取り込む。
fn sha256_stanza_lines<'a>(
    lines: &'a [&'a str],
    sha_index: usize,
    rest: &'a str,
) -> impl Iterator<Item = &'a str> {
    let continuations =
        lines
            .iter()
            .skip(sha_index + 1)
            .scan(line_continues(rest), |prev_continues, line| {
                if !*prev_continues && !starts_with_arch_label(line) {
                    return None;
                }
                *prev_continues = line_continues(line);
                Some(*line)
            });
    std::iter::once(rest).chain(continuations)
}

/// スタンザ範囲の 1 行に `:no_check` トークンが語境界で現れるかの純粋判定。
fn stanza_has_no_check(line: &str) -> bool {
    line.match_indices(":no_check").any(|(start, matched)| {
        line[start + matched.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
    })
}

/// 行末（trim 後）が `,` で終わる＝次行へ続くかの純粋判定（arch 別 sha256 の継続検出に使う）。
fn line_continues(line: &str) -> bool {
    line.trim_end().ends_with(',')
}

/// 行頭（trim 後）が arch ラベル `<ident>:`（`arm:` / `intel:` / `x86_64:` 等）で始まるかの純粋判定。
fn starts_with_arch_label(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed
        .find(':')
        .map(|colon| &trimmed[..colon])
        .is_some_and(|label| {
            !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// homebrew-cask リポジトリ内で cask `.rb` が置かれる `Casks/` 配下の subdir を返す純粋関数。font cask は
/// `font/font-<X>`（`<X>` は `font-` の次の 1 文字を小文字化）、非 font cask は cask 名の先頭文字（小文字化）。
///
/// brew tap rev 差分の `.rb` 取得（[`cask_rb_url`]）と cask homepage/url ヒント取得（[`super::notes`] の cask URL
/// 解決）で同じ subdir 規則を共有し、取得先 path のずれを防ぐ正本とする。
pub(super) fn cask_subdir(name: &str) -> String {
    match name
        .strip_prefix("font-")
        .and_then(|rest| rest.chars().next())
    {
        Some(c) => format!("font/font-{}", c.to_ascii_lowercase()),
        None => name
            .chars()
            .next()
            .map(|c| c.to_ascii_lowercase().to_string())
            .unwrap_or_default(),
    }
}

/// `raw.githubusercontent.com` の cask `.rb` 取得 URL を構築する純粋関数。subdir 規則は [`cask_subdir`] に従う。
fn cask_rb_url(rev: &str, name: &str) -> String {
    let subdir = cask_subdir(name);
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
fn parse_cask_list(nix: &str) -> Result<Vec<String>> {
    let Some(after) = nix.split_once("casks = [").map(|(_, rest)| rest) else {
        bail!(
            "`homebrew.nix` に `casks = [` ブロックが見つからない。brew 差分抽出規約が壊れているため停止する（fail-closed）"
        );
    };
    let block = after
        .split_once(']')
        .map(|(block, _)| block)
        .unwrap_or(after);
    Ok(block
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    //! cask list 抽出・cask URL 構築（letter/font subdir）・version 解析・版差分（auto_updates も追跡・
    //! upgraded/downgraded・ノイズ除去）・`sha256 :no_check`・外部取得失敗の fail-closed を network 抜きで固定する。

    use super::*;
    use std::collections::BTreeMap;

    /// map に本文があれば返し、無ければ外部取得 failure を返す fetch seam。
    ///
    /// HTTP status をテスト用に Cask 不在へ変換しない。production と同じく未取得は error のまま呼出元へ渡す。
    fn body_or_error(map: &BTreeMap<String, String>, url: &str) -> Result<String> {
        map.get(url)
            .cloned()
            .with_context(|| format!("fixture has no cask body for `{url}`"))
    }

    #[test]
    fn parse_cask_list_extracts_quoted_names() -> Result<()> {
        let nix = "  casks = [\n    \"azookey\"\n    \"bitwarden\"\n    \"font-cica\"\n  ];\n";
        assert_eq!(parse_cask_list(nix)?, ["azookey", "bitwarden", "font-cica"]);
        let err = parse_cask_list("no casks here")
            .expect_err("missing casks block must fail closed")
            .to_string();
        assert!(err.contains("casks = ["), "{err}");
        Ok(())
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
        let merged: BTreeMap<String, String> = old
            .iter()
            .chain(new.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let fetch = |url: &str| body_or_error(&merged, url);
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
        let fetch = |url: &str| -> Result<String> {
            if url == cask_rb_url("new", "loose") {
                Ok("cask \"loose\" do\n  version \"1.0\"\n  sha256 :no_check\nend\n".to_string())
            } else {
                Ok(version_rb("1.0"))
            }
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
        // arch 別 sha256（同一行）の `:no_check` も未固定として拾う。
        assert!(has_no_check_sha256(
            "  sha256 arm: :no_check, intel: \"abc123\"\n"
        ));
        assert!(has_no_check_sha256(
            "  sha256 arm: \"abc123\", intel: :no_check\n"
        ));
        // arch 別 sha256（継続行）の `:no_check` も拾う。
        assert!(has_no_check_sha256(
            "  sha256 arm:   \"abc123\",\n         intel: :no_check\n"
        ));
        assert!(has_no_check_sha256(
            "  sha256 arm:   :no_check,\n         intel: \"abc123\"\n"
        ));
        // arch 別だが両 arch とも実 checksum 固定なら未固定ではない。
        assert!(!has_no_check_sha256(
            "  sha256 arm:   \"aaa\",\n         intel: \"bbb\"\n"
        ));
        // `sha256` 直後が別トークン（`sha256sums` 等）の行は対象外。
        assert!(!has_no_check_sha256("  sha256sum :no_check\n"));
        // sha256 スタンザ外（後続の無関係行）の `:no_check` 偶発一致は拾わない。
        assert!(!has_no_check_sha256(
            "  sha256 \"abc123\"\n  desc \":no_check はラベルではない\"\n"
        ));
    }

    #[test]
    fn diff_casks_propagates_new_rev_fetch_failure_without_resource_state_translation() {
        // HTTP error を Added/Removed/差分なしへ変換せず、外部取得 failure として停止する。
        let nix = "casks = [ \"flaky\" ];";
        let fetch = |url: &str| -> Result<String> {
            if url == cask_rb_url("old", "flaky") {
                Ok(version_rb("1.0"))
            } else {
                bail!("fixture HTTP status 404")
            }
        };
        let error = diff_casks(nix, "old", "new", &fetch)
            .expect_err("new rev fetch failure must be propagated");
        assert!(
            error.to_string().contains("flaky"),
            "error should name the cask: {error}"
        );
        assert!(
            error.chain().any(|cause| cause.to_string().contains("404")),
            "error chain should retain the external failure: {error:?}"
        );
    }

    #[test]
    fn diff_casks_propagates_old_rev_fetch_failure_without_resource_state_translation() {
        // old rev の失敗も、差分なし・Added へ変換せず停止する。
        let nix = "casks = [ \"flaky\" ];";
        let fetch = |url: &str| -> Result<String> {
            if url == cask_rb_url("new", "flaky") {
                Ok(version_rb("2.0"))
            } else {
                bail!("fixture transport failure")
            }
        };
        let error = diff_casks(nix, "old", "new", &fetch)
            .expect_err("old rev fetch failure must be propagated");
        assert!(
            error.to_string().contains("flaky"),
            "error should name the cask: {error}"
        );
        assert!(
            error.to_string().contains("old"),
            "error should preserve old-rev context: {error}"
        );
        assert!(
            error
                .chain()
                .any(|cause| cause.to_string().contains("transport failure")),
            "error chain should retain the external failure: {error:?}"
        );
    }

    #[test]
    fn diff_casks_rejects_unversioned_cask_instead_of_marking_it_removed() {
        let nix = "casks = [ \"latest\" ];";
        let fetch = |url: &str| -> Result<String> {
            if url == cask_rb_url("old", "latest") {
                Ok(version_rb("1.0"))
            } else {
                Ok("cask \"latest\" do\n  version :latest\nend\n".to_string())
            }
        };
        let error = diff_casks(nix, "old", "new", &fetch)
            .expect_err("unversioned Cask must not be recorded as Removed")
            .to_string();
        assert!(error.contains("latest"), "{error}");
        assert!(error.contains("quoted `version"), "{error}");
    }

    fn version_rb(version: &str) -> String {
        format!("cask \"x\" do\n  version \"{version}\"\nend\n")
    }
}

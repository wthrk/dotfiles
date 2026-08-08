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
use super::notes::{FetchOutcome, safe_https_fetch_outcome};
use super::wire::{ChangeKind, is_allowed_url};
use crate::Result;

/// cask `.rb` 取得 seam の 3 値結果（本文あり / 明確な不在=404 / それ以外の取得不能）。
///
/// 取得不能（接続失敗・5xx・429 等）を不在と区別しないと、一過性障害を「削除」と誤確定しうる。new rev の
/// `NotFound` は宣言 cask が bump 後 tap に存在しない状態なので fail-closed にし、old rev の `NotFound` だけを
/// `Added` 判定の不在根拠として使う。`Unavailable` は rev により倒し分ける（new rev は fail-closed の `Err`、old
/// rev も fail-closed）ため、本文の有無に加えて 404 かどうかを seam が運ぶ。
pub(crate) enum CaskFetch {
    /// cask `.rb` 本文を取得した。
    Body(String),
    /// 明確な不在（HTTP 404）。new rev でこれになった宣言 cask は fail-closed の停止根拠になる。
    NotFound,
    /// 取得不能（接続失敗・5xx・429・404 以外の失敗）。new rev では固定性を確認できず、old rev では old
    /// version を確定できないため、どちらも brew 更新履歴の欠落を避ける fail-closed の停止根拠にする。
    Unavailable,
}

/// 宣言 cask の old→new tap rev 版差分を算出する。
///
/// `casks_nix` は `nix/modules/homebrew.nix` のテキスト（`casks = [ ... ]` を抽出）。各 cask の `version` を
/// 両 rev の cask `.rb` から取り、版変化のあるものだけ [`VersionDelta`] にする。版変更なしは捨てる（ノイズ抑制）。
/// old rev 取得不能は差分なしとして扱わず fail-closed にする。`greedyCasks = true` で全 cask が無人 upgrade 対象になるため `auto_updates true` の
/// cask も追跡する。new rev の `.rb` が `sha256 :no_check` の未固定成果物なら、または new rev が取得不能で固定性を
/// 確認できないなら fail-closed（`Err`）にする（[`assert_pinned`] / [`cask_delta`]）。
/// `fetch` は cask `.rb` 取得 seam（本番は reqwest、テストは fake）。
pub(crate) fn diff_casks(
    casks_nix: &str,
    rev_old: &str,
    rev_new: &str,
    fetch: &dyn Fn(&str) -> Result<CaskFetch>,
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
/// new rev が `Unavailable`（404 以外の取得不能＝接続失敗・5xx・429 等）なら、固定性（`sha256 :no_check` 検査）を
/// 確認できないまま greedy upgrade が未固定成果物を取り込みうるため `Err` で record を失敗させる（fail-closed:
/// 固定性を確認できないなら PR を作らせない）。old rev が `Unavailable` のときも差分を確定できず、brew-only
/// 更新の履歴欠落につながるため `Err` で record を失敗させる。new rev が 404
/// なら宣言側がまだ同じ PR で直せない cask 欠落なので、`Removed` として記録せず fail-closed にする。new rev の
/// `.rb` 本文に対しては成果物固定を [`assert_pinned`] で検査し、`sha256 :no_check` なら fail-closed。
fn cask_delta(
    rev_old: &str,
    rev_new: &str,
    name: String,
    fetch: &dyn Fn(&str) -> Result<CaskFetch>,
) -> Result<Option<VersionDelta>> {
    let Some(new) = cask_version_pinned(rev_new, &name, fetch)? else {
        // new rev が取得不能（`Unavailable`）→ 固定性を確認できないため record を失敗させる（fail-closed）。
        bail!(
            "cask `{name}` の new rev `.rb` を取得できなかった（接続失敗・5xx・429 等）。固定性\
             （`sha256 :no_check` 検査）を確認できないため停止する（fail-closed: 取得障害時に未固定成果物の\
             混入を許さない）"
        );
    };
    let Some(old) = cask_version(rev_old, &name, fetch)? else {
        bail!(
            "cask `{name}` の old rev `.rb` を取得できなかった（接続失敗・5xx・429 等）。\
             old version を確定できず brew 更新履歴が欠落しうるため停止する（fail-closed）"
        );
    };
    let change = match (&old, &new) {
        (CaskState::Absent, CaskState::Absent) => return Ok(None),
        (CaskState::Absent, CaskState::Version(_)) => ChangeKind::Added,
        (CaskState::Version(_), CaskState::Absent) => ChangeKind::Removed,
        (CaskState::Version(old_v), CaskState::Version(new_v)) => {
            match version_ordering(old_v, new_v) {
                std::cmp::Ordering::Equal => return Ok(None),
                std::cmp::Ordering::Less => ChangeKind::Upgraded,
                std::cmp::Ordering::Greater => ChangeKind::Downgraded,
            }
        }
    };
    Ok(Some(VersionDelta {
        name,
        old: old.into_version(),
        new: new.into_version(),
        change,
        source: DeltaSource::BrewTap,
        repo: None,
        notes_source: None,
        homepage: None,
    }))
}

/// 取得不能（`Unavailable`）を除いた、ある rev における cask の状態（版あり / 明確な不在=404）。
enum CaskState {
    /// `version "..."` を取得した。
    Version(String),
    /// 明確な不在（404、または `.rb` に version 行が無い）。
    Absent,
}

impl CaskState {
    /// 版差分記録用に `Option<String>` へ変換する（`Absent`=`None`）。
    fn into_version(self) -> Option<String> {
        match self {
            CaskState::Version(version) => Some(version),
            CaskState::Absent => None,
        }
    }
}

/// 本番経路: reqwest で cask `.rb` を取得し 3 値（本文 / 404 / 取得不能）の [`CaskFetch`] へ翻訳する fetch seam。
///
/// 許可外 URL は構造的に取得対象外＝明確な不在（`NotFound`）とみなす。取得自体は [`safe_https_fetch_outcome`]
/// （redirect 不追従・https 限定・有界本文）の status から 404 とその他失敗を区別する。
pub(crate) fn fetch_cask_rb(url: &str) -> Result<CaskFetch> {
    if !is_allowed_url(url) {
        return Ok(CaskFetch::NotFound);
    }
    Ok(match safe_https_fetch_outcome(url)? {
        FetchOutcome::Body(body) => CaskFetch::Body(body),
        FetchOutcome::NotFound => CaskFetch::NotFound,
        FetchOutcome::Unavailable => CaskFetch::Unavailable,
    })
}

/// 単一 cask の tap rev における状態を `.rb` から取得する（`Unavailable`=取得不能なら `None` で fail-closed 要求）。
///
/// `Body` は version 行があれば `Version`、無ければ `Absent`。`NotFound`（404）は `Absent`。`Unavailable`
/// （接続失敗・5xx・429 等）は呼び出し側で fail-closed させるため `None` を返す。
fn cask_version(
    rev: &str,
    name: &str,
    fetch: &dyn Fn(&str) -> Result<CaskFetch>,
) -> Result<Option<CaskState>> {
    let url = cask_rb_url(rev, name);
    Ok(match fetch(&url)? {
        CaskFetch::Body(rb) => Some(cask_state_of(&rb)),
        CaskFetch::NotFound => Some(CaskState::Absent),
        CaskFetch::Unavailable => None,
    })
}

/// new rev 用: `.rb` を取得し成果物固定を [`assert_pinned`] で検査してから状態（版/不在）を決める。
///
/// greedy 有効化（`homebrew.nix` の `greedyCasks = true`）で全 cask が無人 upgrade 対象になるため、未固定成果物
/// （`sha256 :no_check`）が new rev に現れたら fail-closed にし、外部成果物の再現性なき無人差し替えを阻む。404
/// （`NotFound`）も宣言 cask 欠落を示すため fail-closed にする。取得不能（`Unavailable`）は固定性を確認できないため
/// `None` を返し、呼び出し側（[`cask_delta`]）が record を失敗させる（fail-closed: 取得障害の夜に未固定 cask の
/// 混入を見逃さない）。
fn cask_version_pinned(
    rev: &str,
    name: &str,
    fetch: &dyn Fn(&str) -> Result<CaskFetch>,
) -> Result<Option<CaskState>> {
    let url = cask_rb_url(rev, name);
    match fetch(&url)? {
        CaskFetch::Body(rb) => assert_pinned(name, &rb).map(|()| Some(cask_state_of(&rb))),
        CaskFetch::NotFound => {
            bail!(
                "cask `{name}` は new rev `{rev}` の homebrew-cask tap に存在しない（404）。\
                 `homebrew.nix` の宣言 cask として残っているため、Removed として履歴記録せず停止する\
                 （fail-closed: 宣言側を人手で修正すること）"
            );
        }
        CaskFetch::Unavailable => Ok(None),
    }
}

/// 取得済み `.rb` 本文を cask の状態へ翻訳する純粋関数（version 行があれば `Version`、無ければ `Absent`）。
fn cask_state_of(rb: &str) -> CaskState {
    match parse_cask_version(rb) {
        Some(version) => CaskState::Version(version),
        None => CaskState::Absent,
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

/// cask `.rb` 本文から最初の `version "..."` を取り出す純粋関数（`version :latest` 等は非対象）。
///
/// `version :latest` を版差分に翻訳できないことは可視化の穴にならない。Homebrew の cask audit は
/// `version :latest` に `sha256 :no_check` を要求するため、`:latest` の cask は [`assert_pinned`] で
/// fail-closed になり、宣言 cask（`homebrew.nix` の `casks`）として存在できない。
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
    //! added/removed/upgraded・ノイズ除去）・`sha256 :no_check` の fail-closed を network 抜きで固定する。

    use super::*;
    use std::collections::BTreeMap;

    /// map に本文があれば `Body`、無ければ `NotFound`（明確な不在）を返す fetch seam（取得不能を出さない）。
    fn body_or_not_found(map: &BTreeMap<String, String>, url: &str) -> CaskFetch {
        match map.get(url) {
            Some(body) => CaskFetch::Body(body.clone()),
            None => CaskFetch::NotFound,
        }
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
        let fetch = |url: &str| -> Result<CaskFetch> { Ok(body_or_not_found(&merged, url)) };
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
        let fetch = |url: &str| -> Result<CaskFetch> {
            Ok(if url == cask_rb_url("new", "loose") {
                CaskFetch::Body(
                    "cask \"loose\" do\n  version \"1.0\"\n  sha256 :no_check\nend\n".to_string(),
                )
            } else {
                CaskFetch::NotFound
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
    fn diff_casks_marks_added_from_old_not_found() -> Result<()> {
        // old 不在(404)→new 版ありは Added。new 側 404 は宣言 cask 欠落として別テストで fail-closed にする。
        let nix = "casks = [ \"newcask\" ];";
        let fetch = |url: &str| -> Result<CaskFetch> {
            Ok(if url == cask_rb_url("new", "newcask") {
                CaskFetch::Body(version_rb("3.0"))
            } else {
                CaskFetch::NotFound
            })
        };
        let deltas = diff_casks(nix, "old", "new", &fetch)?;
        let added = deltas.iter().find(|d| d.name == "newcask");
        assert_eq!(added.map(|d| d.change), Some(ChangeKind::Added));
        Ok(())
    }

    #[test]
    fn diff_casks_fails_closed_when_new_rev_unavailable() {
        // new rev の `.rb` が取得不能（5xx/429/接続失敗＝`Unavailable`）なら、固定性（`sha256 :no_check` 検査）を
        // 確認できないため record を失敗させる（fail-closed: 取得障害の夜に未固定 cask の混入を許さない）。
        let nix = "casks = [ \"flaky\" ];";
        let fetch = |url: &str| -> Result<CaskFetch> {
            Ok(if url == cask_rb_url("old", "flaky") {
                CaskFetch::Body(version_rb("1.0"))
            } else {
                // new rev は取得不能。
                CaskFetch::Unavailable
            })
        };
        let error = diff_casks(nix, "old", "new", &fetch)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("flaky"),
            "error should name the cask: {error}"
        );
        assert!(
            error.contains("fail-closed"),
            "error should cite fail-closed: {error}"
        );
    }

    #[test]
    fn diff_casks_fails_closed_when_declared_cask_is_not_found_at_new_rev() {
        // new rev が明確な不在（404＝`NotFound`）なら、宣言 cask を Removed として履歴記録せず停止する。
        let nix = "casks = [ \"gone\" ];";
        let fetch = |url: &str| -> Result<CaskFetch> {
            Ok(if url == cask_rb_url("old", "gone") {
                CaskFetch::Body(version_rb("1.0"))
            } else {
                CaskFetch::NotFound
            })
        };
        let error = diff_casks(nix, "old", "new", &fetch)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("gone"),
            "error should name the cask: {error}"
        );
        assert!(
            error.contains("404"),
            "error should cite not found status: {error}"
        );
        assert!(
            error.contains("Removed"),
            "error should say it did not record Removed: {error}"
        );
    }

    #[test]
    fn diff_casks_fails_closed_when_old_rev_unavailable() {
        // old rev が取得不能なら、new に版があっても差分なしとして silent skip せず fail-closed にする。
        let nix = "casks = [ \"flaky\" ];";
        let fetch = |url: &str| -> Result<CaskFetch> {
            Ok(if url == cask_rb_url("new", "flaky") {
                CaskFetch::Body(version_rb("2.0"))
            } else {
                CaskFetch::Unavailable
            })
        };
        let error = diff_casks(nix, "old", "new", &fetch)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("flaky"),
            "error should name the cask: {error}"
        );
        assert!(
            error.contains("old rev"),
            "error should cite old rev: {error}"
        );
        assert!(
            error.contains("fail-closed"),
            "error should cite fail-closed: {error}"
        );
    }

    fn version_rb(version: &str) -> String {
        format!("cask \"x\" do\n  version \"{version}\"\nend\n")
    }
}

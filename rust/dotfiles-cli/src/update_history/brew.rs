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
/// 取得不能（接続失敗・5xx・429 等）を不在と区別しないと、一過性障害を「削除」と誤確定しうる。`Removed` 確定は
/// `NotFound`（明確な不在）にだけ許す。`Unavailable` は rev により倒し分ける（new rev は fail-closed の `Err`、old
/// rev は安全側のスキップ）ため、本文の有無に加えて 404 かどうかを seam が運ぶ。
pub(crate) enum CaskFetch {
    /// cask `.rb` 本文を取得した。
    Body(String),
    /// 明確な不在（HTTP 404）。new rev でこれになった cask は削除確定の根拠になる。
    NotFound,
    /// 取得不能（接続失敗・5xx・429・404 以外の失敗）。new rev では固定性を確認できないため record を失敗させ、old
    /// rev では安全側（差分なし扱い）でスキップする。
    Unavailable,
}

/// 宣言 cask の old→new tap rev 版差分を算出する。
///
/// `casks_nix` は `nix/modules/homebrew.nix` のテキスト（`casks = [ ... ]` を抽出）。各 cask の `version` を
/// 両 rev の cask `.rb` から取り、版変化のあるものだけ [`VersionDelta`] にする。old rev 取得不能 / 版変更なし
/// は捨てる（ノイズ抑制）。`greedyCasks = true` で全 cask が無人 upgrade 対象になるため `auto_updates true` の
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
    parse_cask_list(casks_nix)
        .into_iter()
        .map(|name| cask_delta(rev_old, rev_new, name, fetch))
        .collect::<Result<Vec<Option<VersionDelta>>>>()
        .map(|deltas| deltas.into_iter().flatten().collect())
}

/// 1 cask の old/new 版を取得し、版変化があれば [`VersionDelta`] を組む（old 取得不能/版不変は `None`）。
///
/// new rev が `Unavailable`（404 以外の取得不能＝接続失敗・5xx・429 等）なら、固定性（`sha256 :no_check` 検査）を
/// 確認できないまま greedy upgrade が未固定成果物を取り込みうるため `Err` で record を失敗させる（fail-closed:
/// 固定性を確認できないなら PR を作らせない）。old rev が `Unavailable` のときは new の固定性は確認済みで、old が
/// 読めないのは安全側（差分なし扱い）なので差分判定をスキップ（版変化なし扱いの `None`）する。`Removed` 確定は old
/// が版あり・new が明確な不在（404=`Absent`）のときだけ許す。new rev の `.rb` 本文に対しては成果物固定を
/// [`assert_pinned`] で検査し、`sha256 :no_check` なら fail-closed。
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
        // old rev が取得不能（`Unavailable`）→ new の固定性は確認済みで old が読めないのは安全側＝差分なし扱い。
        return Ok(None);
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

/// 単一 cask の tap rev における状態を `.rb` から取得する（`Unavailable`=取得不能なら `None` でスキップ要求）。
///
/// `Body` は version 行があれば `Version`、無ければ `Absent`。`NotFound`（404）は `Absent`。`Unavailable`
/// （接続失敗・5xx・429 等）は版差分判定をスキップさせるため `None` を返す。
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
/// （`NotFound`）は明確な不在＝`Absent`。取得不能（`Unavailable`）は固定性を確認できないため `None` を返し、呼び出し
/// 側（[`cask_delta`]）が record を失敗させる（fail-closed: 取得障害の夜に未固定 cask の混入を見逃さない）。
fn cask_version_pinned(
    rev: &str,
    name: &str,
    fetch: &dyn Fn(&str) -> Result<CaskFetch>,
) -> Result<Option<CaskState>> {
    let url = cask_rb_url(rev, name);
    match fetch(&url)? {
        CaskFetch::Body(rb) => assert_pinned(name, &rb).map(|()| Some(cask_state_of(&rb))),
        CaskFetch::NotFound => Ok(Some(CaskState::Absent)),
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

    /// map に本文があれば `Body`、無ければ `NotFound`（明確な不在）を返す fetch seam（取得不能を出さない）。
    fn body_or_not_found(map: &BTreeMap<String, String>, url: &str) -> CaskFetch {
        match map.get(url) {
            Some(body) => CaskFetch::Body(body.clone()),
            None => CaskFetch::NotFound,
        }
    }

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
    }

    #[test]
    fn diff_casks_marks_added_and_removed() -> Result<()> {
        // newcask: old 不在(404)→new 版あり=Added。oldcask: old 版あり→new 不在(404)=Removed。
        let nix = "casks = [ \"newcask\" \"oldcask\" ];";
        let fetch = |url: &str| -> Result<CaskFetch> {
            Ok(if url == cask_rb_url("new", "newcask") {
                CaskFetch::Body(version_rb("3.0"))
            } else if url == cask_rb_url("old", "oldcask") {
                CaskFetch::Body(version_rb("1.0"))
            } else {
                CaskFetch::NotFound
            })
        };
        let deltas = diff_casks(nix, "old", "new", &fetch)?;
        let added = deltas.iter().find(|d| d.name == "newcask");
        let removed = deltas.iter().find(|d| d.name == "oldcask");
        assert_eq!(added.map(|d| d.change), Some(ChangeKind::Added));
        assert_eq!(removed.map(|d| d.change), Some(ChangeKind::Removed));
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
    fn diff_casks_marks_removed_only_on_explicit_not_found() -> Result<()> {
        // new rev が明確な不在（404＝`NotFound`）で old が版ありのときだけ Removed を確定する。
        let nix = "casks = [ \"gone\" ];";
        let fetch = |url: &str| -> Result<CaskFetch> {
            Ok(if url == cask_rb_url("old", "gone") {
                CaskFetch::Body(version_rb("1.0"))
            } else {
                CaskFetch::NotFound
            })
        };
        let deltas = diff_casks(nix, "old", "new", &fetch)?;
        assert_eq!(
            deltas.iter().find(|d| d.name == "gone").map(|d| d.change),
            Some(ChangeKind::Removed)
        );
        Ok(())
    }

    #[test]
    fn diff_casks_skips_when_old_rev_unavailable() -> Result<()> {
        // old rev が取得不能なら、new に版があっても Added と誤確定せずスキップする（両側 fail-closed）。
        let nix = "casks = [ \"flaky\" ];";
        let fetch = |url: &str| -> Result<CaskFetch> {
            Ok(if url == cask_rb_url("new", "flaky") {
                CaskFetch::Body(version_rb("2.0"))
            } else {
                CaskFetch::Unavailable
            })
        };
        let deltas = diff_casks(nix, "old", "new", &fetch)?;
        assert!(
            deltas.is_empty(),
            "old rev 取得不能はスキップする: {deltas:?}"
        );
        Ok(())
    }

    fn version_rb(version: &str) -> String {
        format!("cask \"x\" do\n  version \"{version}\"\nend\n")
    }
}

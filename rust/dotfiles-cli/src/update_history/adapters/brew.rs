//! `BrewVersionDiffPort` を Homebrew tap rev の formula/cask 版差分ファイルへ接続する adapter。
//!
//! brew 版は tap 自体を flake input rev で pin しているため、各 formula/cask の版は old/new tap rev が
//! 提供するファイル内容で決まり、ライブ `brew` 問い合わせ無しに決定論的に差分できる。本 adapter は CI が
//! old/new tap rev から事前算出した版差分ファイル（行ごとに `name<TAB>old<TAB>new`）を読み、[`VersionDelta`]
//! の `BrewTap` 系統へ翻訳する境界である。`∅` は版不在（added/removed）を表す。
//!
//! tap source path（差分ファイル）が与えられない／読めない実行環境では、record を失敗させず brew 差分を
//! 空で返す（「差分取得不能はフォールバックして version+notes_url へ縮退」のプラン契約に沿う graceful
//! degradation）。版比較規則・差分種別の業務意味は domain rule（[`crate::update_history::domain::diff`]）に
//! 委ね、本 adapter は「ファイル読み取りと行→delta 翻訳」という外部 I/O 翻訳だけを担う。

use std::path::PathBuf;

use crate::Result;
use crate::update_history::domain::diff::{DeltaSource, VersionDelta};
use crate::update_history::domain::wire::ChangeKind;
use crate::update_history::ports::BrewVersionDiffPort;

/// 版不在を示す記号（nix `diff-closures` と同じ `∅` を用いる）。
const ABSENT: &str = "∅";

/// Homebrew tap rev の版差分解決を `BrewVersionDiffPort` 契約へ翻訳する adapter。
///
/// `diff_file` は CI が old/new tap rev から事前算出した版差分ファイル（`name<TAB>old<TAB>new`）の path。
/// `None` または読めない場合は brew 差分を空で返す（縮退）。
#[derive(Default)]
pub(in crate::update_history) struct BrewTapDiffAdapter {
    /// tap rev 版差分ファイルの path。未設定なら brew 差分は空。
    diff_file: Option<PathBuf>,
}

impl BrewTapDiffAdapter {
    /// tap rev 版差分ファイル path を束ねた adapter を作る。`None` で brew 差分を縮退（空）にする。
    pub(in crate::update_history) fn new(diff_file: Option<PathBuf>) -> Self {
        Self { diff_file }
    }

    /// 差分ファイル本文を `name<TAB>old<TAB>new` 行として `BrewTap` 系統の delta へ翻訳する。
    ///
    /// 空行・3 列に満たない行は無視する。`∅` は版不在として `None` にし、不在位置から change 種別を確定する
    /// （`∅→x`=Added、`x→∅`=Removed、両側存在=Upgraded）。版比較の業務意味づけは domain rule に委ねる。
    ///
    /// **版変更なし（`old==new`）行の除外**（F5 ノイズ抑制）: CI 側でも落とすが、差分源の品質に依存せず
    /// adapter 側でも `old` と `new` がともに存在して等しい行を捨てる。版が変わっていない cask は「更新」では
    /// なく、記録・表示に出すと no-op エントリのノイズになるため、ここで二重に防ぐ。
    fn parse_diff(text: &str) -> Vec<VersionDelta> {
        text.lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let name = fields.next()?.trim();
                let old_raw = fields.next()?.trim();
                let new_raw = fields.next()?.trim();
                if name.is_empty() {
                    return None;
                }
                let old = version_or_absent(old_raw);
                let new = version_or_absent(new_raw);
                // 版変更なし（両側存在かつ等しい）は更新ではないため捨てる（F5 ノイズ抑制）。
                if let (Some(old_v), Some(new_v)) = (&old, &new)
                    && old_v == new_v
                {
                    return None;
                }
                let change = match (&old, &new) {
                    (None, Some(_)) => ChangeKind::Added,
                    (Some(_), None) => ChangeKind::Removed,
                    _ => ChangeKind::Upgraded,
                };
                Some(VersionDelta {
                    name: name.to_string(),
                    old,
                    new,
                    change,
                    source: DeltaSource::BrewTap,
                    // brew はノート URL を cask base + name で解決するため delta には取得元を持たせない。
                    repo: None,
                    notes_source: None,
                    homepage: None,
                })
            })
            .collect()
    }
}

impl BrewVersionDiffPort for BrewTapDiffAdapter {
    fn diff_brew_versions(&self, _old_rev: &str, _new_rev: &str) -> Result<Vec<VersionDelta>> {
        // 版差分ファイルが解決できない／読めない実行環境では brew 差分を空で返す（縮退）。
        let Some(path) = &self.diff_file else {
            return Ok(Vec::new());
        };
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse_diff(&text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }
}

/// `∅`（不在）または空を `None` に、それ以外を版文字列として返す。
fn version_or_absent(value: &str) -> Option<String> {
    if value.is_empty() || value == ABSENT {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! 版差分ファイルの行→`BrewTap` delta 翻訳（種別確定・`∅` 扱い・不正行無視）を固定する。

    use super::BrewTapDiffAdapter;
    use crate::update_history::domain::diff::DeltaSource;
    use crate::update_history::domain::wire::ChangeKind;

    #[test]
    fn parses_tab_separated_brew_versions() {
        let text = "firefox\t120.0\t121.0\nslack\t∅\t4.36\nold-cask\t1.0\t∅\nbad line\n";
        let deltas = BrewTapDiffAdapter::parse_diff(text);
        assert_eq!(deltas.len(), 3);

        assert_eq!(deltas[0].name, "firefox");
        assert_eq!(deltas[0].old.as_deref(), Some("120.0"));
        assert_eq!(deltas[0].new.as_deref(), Some("121.0"));
        assert_eq!(deltas[0].change, ChangeKind::Upgraded);
        assert_eq!(deltas[0].source, DeltaSource::BrewTap);

        assert_eq!(deltas[1].change, ChangeKind::Added);
        assert_eq!(deltas[1].old, None);

        assert_eq!(deltas[2].change, ChangeKind::Removed);
        assert_eq!(deltas[2].new, None);
    }

    #[test]
    fn ignores_blank_and_short_lines() {
        assert!(BrewTapDiffAdapter::parse_diff("\n\nonlyname\nname\tonly-old\n").is_empty());
    }

    #[test]
    fn drops_rows_with_unchanged_version() {
        // F5 退行固定: old==new（版変更なし）の cask 行は更新でないため adapter が落とす。実際の更新
        // （版が変わった行・added・removed）だけが残る。
        let text = "unchanged\t4.36\t4.36\nfirefox\t120.0\t121.0\nnew-cask\t∅\t1.0\n";
        let deltas = BrewTapDiffAdapter::parse_diff(text);
        assert_eq!(deltas.len(), 2);
        assert!(
            !deltas.iter().any(|delta| delta.name == "unchanged"),
            "old==new の行は除外する: {deltas:?}"
        );
        assert_eq!(deltas[0].name, "firefox");
        assert_eq!(deltas[1].name, "new-cask");
    }
}

//! `show` が表示するエントリ範囲を決める純粋 domain 関数。
//!
//! catch-up の表示は「適用済み pin（`rev`）より後に記録された全エントリ」を対象にする。ここでは
//! 取得済みエントリ列から、起点 rev と件数上限という表示意図だけで対象部分列を切り出す業務規則を固定する。
//! ファイル読み込み・描画・集約は別責務（adapter / [`super::aggregate`]）であり、本 module は範囲決定だけを担う。

use super::wire::UpdateEntry;

/// 表示対象エントリを起点 rev と件数上限で絞り込み、適用順（最古→最新）の部分列を返す。
///
/// 規則:
/// - `entries` は記録順（最古→最新）で渡す。`nixpkgs_old`/`nixpkgs_new` のチェーンには依存せず、
///   渡された並びを適用順として扱う（source 解決と読み出し順は呼び出し側の責務）。
/// - `rev` が `Some` のとき、その rev を `nixpkgs_old` に持つ最初のエントリ以降（その rev を適用前
///   状態とする catch-up 区間）を起点にする。一致が無ければ「その rev は既に最新」とみなし空を返す。
/// - `rev` が `None` のときは全エントリを起点にする。
/// - `limit` が `Some(n)` のとき、起点側（最古）から最大 n 件に切り詰める。`Some(0)` は空。
///
/// 返値は所有 [`UpdateEntry`] の clone であり、catch-up 集約や severity 再算出へそのまま渡せる。
pub(crate) fn select_entries(
    entries: &[UpdateEntry],
    rev: Option<&str>,
    limit: Option<usize>,
) -> Vec<UpdateEntry> {
    let start = match rev {
        Some(rev) => match entries.iter().position(|entry| entry.nixpkgs_old == rev) {
            Some(index) => index,
            // 起点 rev を適用前状態とするエントリが無い = 既に最新まで適用済み。表示対象なし。
            None => return Vec::new(),
        },
        None => 0,
    };
    let span = &entries[start..];
    match limit {
        Some(limit) => span.iter().take(limit).cloned().collect(),
        None => span.to_vec(),
    }
}

/// 適用後要約のカーソル: 「最後に要約し終えたエントリ」より後のエントリだけを適用順で返す。
///
/// auto 適用後要約を `nixpkgs_old` 起点（[`select_entries`]）で選ぶと brew-only 更新を毎回再表示する。brew
/// tap だけが進み `nixpkgs_old == nixpkgs_new`（= 同一 nixpkgs rev `N`）のエントリが複数できると、要約済み
/// marker も `N` のままになり、次回 `select_entries(..., Some("N"), ...)` が同じ `N -> N` エントリを再選択する
/// ためである（nixpkgs rev では `N -> N` を越えて進めない）。
///
/// 本関数は nixpkgs rev ではなく **履歴エントリの記録時刻 `at`（RFC3339）を単調カーソル**にして `N -> N` を
/// 越える。各エントリの `at` は記録のたびに前進する一意な値（brew-only 夜でも進む）であり、RFC3339 文字列の
/// 辞書順は時系列順に一致する。
///
/// 規則:
/// - `entries` は記録順（最古→最新 = `at` 昇順）で渡す。
/// - `after_at` が `Some(t)` のとき、`at` が `t` より **厳密に後**（`entry.at > t`）のエントリだけを残す
///   （`t` 自身のエントリは要約済みとして除外する。これが `N -> N` 再表示の抑止）。
/// - `after_at` が `None`（初回・marker 未確定）のときは全エントリを対象にする。
/// - `limit` が `Some(n)` のとき、起点側（最古）から最大 n 件に切り詰める。`Some(0)` は空。
///
/// 返値は所有 [`UpdateEntry`] の clone で、catch-up 集約・severity 再算出へそのまま渡せる。
pub(crate) fn select_entries_after(
    entries: &[UpdateEntry],
    after_at: Option<&str>,
    limit: Option<usize>,
) -> Vec<UpdateEntry> {
    let span = entries
        .iter()
        .filter(|entry| match after_at {
            // `at` が marker より厳密に後のエントリだけ（要約済みの `at == marker` を除外）。
            Some(after) => entry.at.as_str() > after,
            None => true,
        })
        .cloned();
    match limit {
        Some(limit) => span.take(limit).collect(),
        None => span.collect(),
    }
}

/// 与えたエントリ列のうち最後（最新）のエントリの `at`（要約済み marker 確定に使う）。
///
/// 空なら `None`。auto 適用後要約は要約「後」にこの `at` を marker へ書き、次回 [`select_entries_after`] の
/// `after_at` に渡す。これにより `N -> N` の brew-only 更新も一度要約したら marker が前進し再表示されない。
pub(crate) fn last_summarized_at(entries: &[UpdateEntry]) -> Option<String> {
    entries.last().map(|entry| entry.at.clone())
}

#[cfg(test)]
mod tests {
    //! 起点 rev からの catch-up 区間切り出しと件数上限の適用を固定する。

    use super::*;
    use crate::update_history::domain::wire::{Severity, UpdateEntry};

    fn entry(old: &str, new: &str) -> UpdateEntry {
        UpdateEntry {
            at: format!("{old}->{new}"),
            nixpkgs_old: old.to_string(),
            nixpkgs_new: new.to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::None,
            overall: String::new(),
            packages: Vec::new(),
        }
    }

    #[test]
    fn rev_resolves_start_when_origin_link_has_empty_packages() {
        // 退行固定（chain 連続性）: 起点 rev r0 を `nixpkgs_old` に持つエントリが空 bump 夜の chain link
        // （packages 空）であっても、`select_entries` はその link を起点に解決し、後続の実更新エントリ
        // r1→r2 まで span に含める（空 link で catch-up 起点が失われない）。
        let entries = [entry("r0", "r1"), entry("r1", "r2")];
        let selected = select_entries(&entries, Some("r0"), None);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].nixpkgs_old, "r0");
        assert_eq!(selected[1].nixpkgs_new, "r2");
    }

    #[test]
    fn no_rev_returns_all_in_order() {
        let entries = [entry("a", "b"), entry("b", "c")];
        let selected = select_entries(&entries, None, None);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].nixpkgs_old, "a");
        assert_eq!(selected[1].nixpkgs_old, "b");
    }

    #[test]
    fn rev_selects_span_from_matching_old() {
        let entries = [entry("a", "b"), entry("b", "c"), entry("c", "d")];
        let selected = select_entries(&entries, Some("b"), None);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].nixpkgs_old, "b");
        assert_eq!(selected[1].nixpkgs_old, "c");
    }

    #[test]
    fn unmatched_rev_returns_empty() {
        let entries = [entry("a", "b")];
        assert!(select_entries(&entries, Some("z"), None).is_empty());
    }

    #[test]
    fn limit_truncates_from_oldest() {
        let entries = [entry("a", "b"), entry("b", "c"), entry("c", "d")];
        let selected = select_entries(&entries, None, Some(2));
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].nixpkgs_old, "a");
        assert_eq!(selected[1].nixpkgs_old, "b");
        assert!(select_entries(&entries, None, Some(0)).is_empty());
    }

    /// `at` を明示できる entry を作る（`at` カーソルの検証用）。`nixpkgs_old==nixpkgs_new` で brew-only 夜を表す。
    fn entry_at(at: &str, nixpkgs: &str) -> UpdateEntry {
        UpdateEntry {
            at: at.to_string(),
            nixpkgs_old: nixpkgs.to_string(),
            nixpkgs_new: nixpkgs.to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::None,
            overall: String::new(),
            packages: Vec::new(),
        }
    }

    #[test]
    fn select_after_at_does_not_redisplay_brew_only_n_to_n_entries() {
        // 退行固定（P2: brew-only 再表示抑止）: nixpkgs rev が動かない（`N -> N`）brew-only 更新が複数あっても、
        // `at` カーソルで要約済みを越えて進む。nixpkgs rev 起点（`select_entries(Some("N"))`）は最初の `N -> N`
        // を毎回再選択するが、`select_entries_after` は要約済み `at` より後だけを選ぶため再表示しない。
        let entries = [
            entry_at("2026-06-01T00:00:00Z", "N"),
            entry_at("2026-06-02T00:00:00Z", "N"),
            entry_at("2026-06-03T00:00:00Z", "N"),
        ];

        // 初回（marker 無し）: 全件対象。要約後 marker = 最後の at。
        let first = select_entries_after(&entries, None, None);
        assert_eq!(first.len(), 3);
        let marker = last_summarized_at(&first).expect("non-empty span has a terminal at");
        assert_eq!(marker, "2026-06-03T00:00:00Z");

        // 2 回目（同じ履歴・新規 brew 更新なし）: marker 以降は空。`N -> N` を再表示しない。
        let second = select_entries_after(&entries, Some(marker.as_str()), None);
        assert!(
            second.is_empty(),
            "要約済み at 以降に新規が無ければ再表示しない: {second:?}"
        );

        // 対照: nixpkgs rev 起点だと最初の `N -> N` を再選択してしまう（旧経路の再表示バグ）。
        let rev_based = select_entries(&entries, Some("N"), None);
        assert_eq!(
            rev_based.len(),
            3,
            "nixpkgs rev 起点は N->N を毎回再選択する（at カーソルが必要な根拠）"
        );
    }

    #[test]
    fn select_after_at_picks_only_newer_entries_then_advances() {
        // marker より後の新規 brew-only 更新だけを選ぶ。要約後は marker がその新規の at へ進む。
        let entries = [
            entry_at("2026-06-01T00:00:00Z", "N"),
            entry_at("2026-06-02T00:00:00Z", "N"),
        ];
        let selected = select_entries_after(&entries, Some("2026-06-01T00:00:00Z"), None);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].at, "2026-06-02T00:00:00Z");
        assert_eq!(
            last_summarized_at(&selected).as_deref(),
            Some("2026-06-02T00:00:00Z")
        );
    }

    #[test]
    fn select_after_at_none_marker_returns_all_and_respects_limit() {
        let entries = [
            entry_at("2026-06-01T00:00:00Z", "N"),
            entry_at("2026-06-02T00:00:00Z", "N"),
            entry_at("2026-06-03T00:00:00Z", "N"),
        ];
        // marker 無し（初回）は全件。
        assert_eq!(select_entries_after(&entries, None, None).len(), 3);
        // limit は最古側から切る。
        let limited = select_entries_after(&entries, None, Some(2));
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].at, "2026-06-01T00:00:00Z");
        // 空 span の marker は None。
        assert_eq!(last_summarized_at(&[]), None);
    }
}

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
}

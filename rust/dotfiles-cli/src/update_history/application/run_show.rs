//! show use case: 履歴を読み、catch-up 範囲を集約し、再算出した重要度ビューを出力する。

use crate::Result;
use crate::update_history::domain::aggregate::aggregate;
use crate::update_history::domain::build::recompute_severity;
use crate::update_history::domain::commands::ShowCommand;
use crate::update_history::domain::selection::{
    last_summarized_at, select_entries, select_entries_after,
};
use crate::update_history::domain::severity::overall_headline;
use crate::update_history::domain::view::HistoryView;
use crate::update_history::domain::wire::{ChangeItem, PackageSourceFilter, UpdateEntry};
use crate::update_history::ports::{HistoryReportPort, HistoryStorePort};

/// 適用済み pin 由来の履歴を読み、起点 rev からの catch-up 区間を集約して重要度連動ビューを出力する。
///
/// 順序制御の理由: 読み出し → 範囲選択（rev/limit）→ catch-up 集約 → severity/overall 再算出の順に
/// するのは、複数 nightly bump を跨いだ適用を「アプリ単位の old→new と重複排除済み変更リスト」へ畳んでから
/// 全体重要度を出すためである。severity/overall は集約後集合に対し record 側と同一の domain 関数で再算出し、
/// 記録時と表示時で重要度規則を二重化しない。`all` による宣言/非宣言の絞り込みは表示意図であり、ここで
/// 適用してから出力境界へ渡す（presentation 整形は adapter に閉じる）。停止条件は各 port の `Err` 伝播。
///
/// 選択は `after_at`（適用後要約の `at` 単調カーソル）があればそれを優先し、無ければ利用者 `show` の `rev`
/// 起点で切り出す（[`select_for_show`]）。
pub(crate) fn run_show<S, R>(command: ShowCommand, store: &S, report: &R) -> Result<()>
where
    S: HistoryStorePort,
    R: HistoryReportPort,
{
    let entries = store.read_entries()?;
    let selected = select_for_show(&entries, &command);
    let view = build_view(&selected, command.all, command.source_filter);
    report.write_history(&view, command.json)
}

/// 適用後要約 use case: `after_at` カーソル以降の更新を集約・表示し、要約し終えた終端 `at` を返す。
///
/// 順序制御の理由: 読み出し → `at` カーソル選択 → catch-up 集約 → severity/overall 再算出 → 出力 → 終端 `at`
/// 確定。auto 経路は nixpkgs rev では `N -> N`（brew-only）を越えられないため、要約済みエントリの `at` を
/// 単調カーソルにして再表示を抑止する。戻り値の終端 `at` を呼び出し側が marker へ書き、次回の `after_at` に
/// 渡すことで、一度要約した brew-only 更新が再表示されない。`show` 経路と同じ集約・severity・表示形式を
/// 共有 domain/helper で使い重複実装しない（use case 間呼び出しはしない）。`None` を返すのは選択範囲が空
/// （新規更新なし）のときで、呼び出し側は marker を進めない。
pub(crate) fn run_applied_summary<S, R>(
    command: ShowCommand,
    store: &S,
    report: &R,
) -> Result<Option<String>>
where
    S: HistoryStorePort,
    R: HistoryReportPort,
{
    let entries = store.read_entries()?;
    let selected = select_entries_after(&entries, command.after_at.as_deref(), command.limit);
    let view = build_view(&selected, command.all, command.source_filter);
    report.write_history(&view, command.json)?;
    // 要約し終えた終端エントリの `at`（次回 `after_at` カーソル）。空 span なら `None`。
    Ok(last_summarized_at(&selected))
}

/// command の `after_at`（優先）/`rev` から表示対象エントリを切り出す。
///
/// `after_at` が `Some` のとき auto 適用後要約の `at` 単調カーソル選択、`None` のとき利用者 `show` の
/// nixpkgs rev 起点選択にフォールバックする（両者は排他に使う）。
fn select_for_show(entries: &[UpdateEntry], command: &ShowCommand) -> Vec<UpdateEntry> {
    match command.after_at.as_deref() {
        Some(after) => select_entries_after(entries, Some(after), command.limit),
        None => select_entries(entries, command.rev.as_deref(), command.limit),
    }
}

/// 選択済みエントリを catch-up 集約し、severity/overall を再算出した表示ビューを組み立てる。
///
/// `all=false` は宣言アプリ中心（既定）、`true` は低レベル依存も含める。`source_filter` は適用後要約を実際に
/// 適用した target に対応する出所だけへ絞る（finding 3368653947。`NixOnly` は brew cask を除外して home 部分適用で
/// 未適用 cask を通知しない。利用者 `show`/全体適用は `All`）。severity/overall は **絞り込み後**集合に対し record
/// 側と同一の domain 関数で再算出し、記録時と表示時で重要度規則を二重化しない（`show`/適用後要約で共有）。
fn build_view(
    selected: &[UpdateEntry],
    all: bool,
    source_filter: PackageSourceFilter,
) -> HistoryView {
    let mut packages = aggregate(selected);
    if !all {
        // 既定は宣言アプリ中心。`--all` 指定時のみ低レベル依存も表示する。
        packages.retain(|package| package.declared);
    }
    // 適用後要約は実際に適用した target に対応する出所だけへ絞る（home 部分適用は nix のみ → cask 誤通知を防ぐ）。
    packages.retain(|package| source_filter.includes(package.source));
    let all_items: Vec<ChangeItem> = packages
        .iter()
        .flat_map(|package| package.change_items.clone())
        .collect();
    let severity = recompute_severity(&all_items);
    let overall = overall_headline(packages.len(), &all_items);
    HistoryView {
        packages,
        severity,
        overall,
    }
}

#[cfg(test)]
mod tests {
    //! show の順序（読み出し → 範囲選択 → 集約 → severity 再算出 → 出力）と `--all` 絞り込みを mock で固定する。

    use super::{run_applied_summary, run_show};
    use crate::update_history::domain::commands::ShowCommand;
    use crate::update_history::domain::wire::{
        ChangeCategory, ChangeItem, ChangeKind, PackageSource, PackageSourceFilter, PackageUpdate,
        Severity, UpdateEntry,
    };
    use crate::update_history::ports::{MockHistoryReportPort, MockHistoryStorePort};

    fn package(name: &str, declared: bool, category: ChangeCategory) -> PackageUpdate {
        package_with_source(name, declared, category, PackageSource::Nix)
    }

    fn package_with_source(
        name: &str,
        declared: bool,
        category: ChangeCategory,
        source: PackageSource,
    ) -> PackageUpdate {
        PackageUpdate {
            name: name.to_string(),
            old: Some("1.0".to_string()),
            new: Some("1.1".to_string()),
            change: ChangeKind::Upgraded,
            declared,
            source,
            notes_url: None,
            change_items: vec![ChangeItem {
                category,
                text: "変更".to_string(),
                ref_url: None,
            }],
        }
    }

    fn entry(packages: Vec<PackageUpdate>) -> UpdateEntry {
        entry_with_revs("a", "b", packages)
    }

    fn entry_with_revs(old: &str, new: &str, packages: Vec<PackageUpdate>) -> UpdateEntry {
        UpdateEntry {
            at: format!("{old}->{new}"),
            nixpkgs_old: old.to_string(),
            nixpkgs_new: new.to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::None,
            overall: String::new(),
            packages,
        }
    }

    fn command(all: bool) -> ShowCommand {
        ShowCommand {
            rev: None,
            after_at: None,
            limit: None,
            json: false,
            all,
            source_filter: PackageSourceFilter::All,
        }
    }

    fn command_from_rev(rev: &str) -> ShowCommand {
        ShowCommand {
            rev: Some(rev.to_string()),
            after_at: None,
            limit: None,
            json: false,
            all: false,
            source_filter: PackageSourceFilter::All,
        }
    }

    fn command_after_at(after_at: &str) -> ShowCommand {
        ShowCommand {
            rev: None,
            after_at: Some(after_at.to_string()),
            limit: None,
            json: false,
            all: false,
            source_filter: PackageSourceFilter::All,
        }
    }

    #[test]
    fn show_aggregates_and_recomputes_then_writes() -> crate::Result<()> {
        let mut store = MockHistoryStorePort::new();
        store.expect_read_entries().times(1).returning(|| {
            Ok(vec![entry(vec![
                package("openssl", true, ChangeCategory::Security),
                package("neovim", true, ChangeCategory::Feature),
            ])])
        });

        let mut report = MockHistoryReportPort::new();
        report
            .expect_write_history()
            .times(1)
            .withf(|view, json| {
                !json
                    && view.severity == Severity::Critical
                    && view.overall == "2アプリ更新: 🔒1 ✨1"
                    && view.packages.len() == 2
            })
            .returning(|_, _| Ok(()));

        run_show(command(false), &store, &report)
    }

    #[test]
    fn show_default_hides_undeclared_packages() -> crate::Result<()> {
        let mut store = MockHistoryStorePort::new();
        store.expect_read_entries().returning(|| {
            Ok(vec![entry(vec![
                package("neovim", true, ChangeCategory::Feature),
                package("libfoo", false, ChangeCategory::Fix),
            ])])
        });

        let mut report = MockHistoryReportPort::new();
        report
            .expect_write_history()
            .withf(|view, _| {
                view.packages.len() == 1
                    && view.packages[0].name == "neovim"
                    && view.severity == Severity::Minor
            })
            .returning(|_, _| Ok(()));

        run_show(command(false), &store, &report)
    }

    #[test]
    fn show_all_includes_undeclared_packages() -> crate::Result<()> {
        let mut store = MockHistoryStorePort::new();
        store.expect_read_entries().returning(|| {
            Ok(vec![entry(vec![
                package("neovim", true, ChangeCategory::Feature),
                package("libfoo", false, ChangeCategory::Fix),
            ])])
        });

        let mut report = MockHistoryReportPort::new();
        report
            .expect_write_history()
            .withf(|view, _| view.packages.len() == 2)
            .returning(|_, _| Ok(()));

        run_show(command(true), &store, &report)
    }

    #[test]
    fn catch_up_resolves_start_across_empty_chain_link() -> crate::Result<()> {
        // 退行固定（chain 連続性）: マシンが nixpkgs r0 に pin され、履歴に r0→r1（packages 空の
        // chain link）と r1→r2（実 packages）がある。`rev=r0` で show すると、`select_entries` は
        // 空 link を跨いで起点 r0 を解決し、r1→r2 の更新を集約して表示する（空集合にならない）。
        // 空 link の存在は package 件数（見出し）を水増ししない（aggregate が package=0 を畳む）。
        let mut store = MockHistoryStorePort::new();
        store.expect_read_entries().times(1).returning(|| {
            Ok(vec![
                // r0→r1: 空 bump 夜の chain link（packages 空）。
                entry_with_revs("r0", "r1", Vec::new()),
                // r1→r2: 実際に適用・記録された更新。
                entry_with_revs(
                    "r1",
                    "r2",
                    vec![package("neovim", true, ChangeCategory::Feature)],
                ),
            ])
        });

        let mut report = MockHistoryReportPort::new();
        report
            .expect_write_history()
            .times(1)
            .withf(|view, _| {
                // 起点 r0 が空 link 越しに解決され、r1→r2 の neovim 更新が表示される。
                view.packages.len() == 1
                    && view.packages[0].name == "neovim"
                    && view.severity == Severity::Minor
                    // 見出しは表示対象（実 package）件数ベース。空 link は水増ししない。
                    && view.overall == "1アプリ更新: ✨1"
            })
            .returning(|_, _| Ok(()));

        run_show(command_from_rev("r0"), &store, &report)
    }

    #[test]
    fn catch_up_returns_empty_when_only_empty_link_selected() -> crate::Result<()> {
        // 空 link だけが選択範囲のとき（その rev 以降に実更新が無い）、集約後 package は 0 件で、
        // 見出しは「0アプリ更新」、severity は None。空 link が利用者表示のノイズにならないことを固定。
        let mut store = MockHistoryStorePort::new();
        store
            .expect_read_entries()
            .times(1)
            .returning(|| Ok(vec![entry_with_revs("r0", "r1", Vec::new())]));

        let mut report = MockHistoryReportPort::new();
        report
            .expect_write_history()
            .times(1)
            .withf(|view, _| {
                view.packages.is_empty()
                    && view.severity == Severity::None
                    && view.overall == "0アプリ更新"
            })
            .returning(|_, _| Ok(()));

        run_show(command_from_rev("r0"), &store, &report)
    }

    /// 明示 `at`（と nixpkgs_old==nixpkgs_new=N）で brew-only 夜を表すエントリを作る。
    fn entry_at(at: &str, packages: Vec<PackageUpdate>) -> UpdateEntry {
        UpdateEntry {
            at: at.to_string(),
            nixpkgs_old: "N".to_string(),
            nixpkgs_new: "N".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::None,
            overall: String::new(),
            packages,
        }
    }

    #[test]
    fn applied_summary_advances_cursor_and_skips_already_summarized_brew_only() -> crate::Result<()>
    {
        // 退行固定（P2: brew-only 再表示抑止）: nixpkgs rev が動かない（`N -> N`）brew-only 更新を、`at`
        // カーソルで一度要約したら再表示しない。1 回目（after_at=None）で全件要約し終端 `at` を marker として
        // 返す。2 回目（after_at=marker）は新規が無ければ空 view（「0アプリ更新」）になり、要約済み更新を
        // 再表示しない。run_applied_summary が要約済み終端 `at` を返すことも固定する。
        let mut store = MockHistoryStorePort::new();
        store.expect_read_entries().times(1).returning(|| {
            Ok(vec![
                entry_at(
                    "2026-06-01T00:00:00Z",
                    vec![package("firefox", true, ChangeCategory::Feature)],
                ),
                entry_at(
                    "2026-06-02T00:00:00Z",
                    vec![package("slack", true, ChangeCategory::Fix)],
                ),
            ])
        });
        let mut report = MockHistoryReportPort::new();
        report
            .expect_write_history()
            .times(1)
            .withf(|view, _| view.packages.len() == 2)
            .returning(|_, _| Ok(()));

        // 1 回目: marker 無し → 全 brew-only 更新を要約。終端 at を返す。
        let cursor = run_applied_summary(command_after_at_none(), &store, &report)?;
        assert_eq!(cursor.as_deref(), Some("2026-06-02T00:00:00Z"));

        // 2 回目: marker = 終端 at。新規が無いので空 view（「0アプリ更新」）。再表示しない。
        let mut store2 = MockHistoryStorePort::new();
        store2.expect_read_entries().times(1).returning(|| {
            Ok(vec![
                entry_at(
                    "2026-06-01T00:00:00Z",
                    vec![package("firefox", true, ChangeCategory::Feature)],
                ),
                entry_at(
                    "2026-06-02T00:00:00Z",
                    vec![package("slack", true, ChangeCategory::Fix)],
                ),
            ])
        });
        let mut report2 = MockHistoryReportPort::new();
        report2
            .expect_write_history()
            .times(1)
            .withf(|view, _| view.packages.is_empty() && view.overall == "0アプリ更新")
            .returning(|_, _| Ok(()));

        let cursor2 =
            run_applied_summary(command_after_at("2026-06-02T00:00:00Z"), &store2, &report2)?;
        // 空 span なので marker は進めない（None）。
        assert_eq!(cursor2, None);
        Ok(())
    }

    #[test]
    fn applied_summary_nix_only_filter_excludes_brew_packages() -> crate::Result<()> {
        // finding 3368653947 退行固定: home 部分適用（`NixOnly`）の要約は brew cask を含めない。集約後に出所で
        // 絞り、severity/overall も絞り込み後集合で再算出する（未適用 cask が件数・重要度に乗らない）。
        let mut store = MockHistoryStorePort::new();
        store.expect_read_entries().times(1).returning(|| {
            Ok(vec![entry(vec![
                package_with_source("neovim", true, ChangeCategory::Feature, PackageSource::Nix),
                package_with_source(
                    "firefox",
                    true,
                    ChangeCategory::Feature,
                    PackageSource::Brew,
                ),
            ])])
        });
        let mut report = MockHistoryReportPort::new();
        report
            .expect_write_history()
            .times(1)
            .withf(|view, _| {
                // nix の neovim だけ残り、brew cask の firefox は除外される。見出しも 1 アプリ。
                view.packages.len() == 1
                    && view.packages[0].name == "neovim"
                    && view.overall == "1アプリ更新: ✨1"
            })
            .returning(|_, _| Ok(()));

        let command = ShowCommand {
            rev: None,
            after_at: None,
            limit: None,
            json: false,
            all: false,
            source_filter: PackageSourceFilter::NixOnly,
        };
        run_applied_summary(command, &store, &report)?;
        Ok(())
    }

    fn command_after_at_none() -> ShowCommand {
        ShowCommand {
            rev: None,
            after_at: None,
            limit: None,
            json: false,
            all: false,
            source_filter: PackageSourceFilter::All,
        }
    }
}

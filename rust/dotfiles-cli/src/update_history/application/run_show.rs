//! show use case: 履歴を読み、catch-up 範囲を集約し、再算出した重要度ビューを出力する。

use crate::Result;
use crate::update_history::domain::aggregate::aggregate;
use crate::update_history::domain::build::recompute_severity;
use crate::update_history::domain::commands::ShowCommand;
use crate::update_history::domain::selection::select_entries;
use crate::update_history::domain::severity::overall_headline;
use crate::update_history::domain::view::HistoryView;
use crate::update_history::domain::wire::ChangeItem;
use crate::update_history::ports::{HistoryReportPort, HistoryStorePort};

/// 適用済み pin 由来の履歴を読み、起点 rev からの catch-up 区間を集約して重要度連動ビューを出力する。
///
/// 順序制御の理由: 読み出し → 範囲選択（rev/limit）→ catch-up 集約 → severity/overall 再算出の順に
/// するのは、複数 nightly bump を跨いだ適用を「アプリ単位の old→new と重複排除済み変更リスト」へ畳んでから
/// 全体重要度を出すためである。severity/overall は集約後集合に対し record 側と同一の domain 関数で再算出し、
/// 記録時と表示時で重要度規則を二重化しない。`all` による宣言/非宣言の絞り込みは表示意図であり、ここで
/// 適用してから出力境界へ渡す（presentation 整形は adapter に閉じる）。停止条件は各 port の `Err` 伝播。
pub(crate) fn run_show<S, R>(command: ShowCommand, store: &S, report: &R) -> Result<()>
where
    S: HistoryStorePort,
    R: HistoryReportPort,
{
    let entries = store.read_entries()?;
    let selected = select_entries(&entries, command.rev.as_deref(), command.limit);
    let mut packages = aggregate(&selected);
    if !command.all {
        // 既定は宣言アプリ中心。`--all` 指定時のみ低レベル依存も表示する。
        packages.retain(|package| package.declared);
    }

    let all_items: Vec<ChangeItem> = packages
        .iter()
        .flat_map(|package| package.change_items.clone())
        .collect();
    let severity = recompute_severity(&all_items);
    let overall = overall_headline(packages.len(), &all_items);

    let view = HistoryView {
        packages,
        severity,
        overall,
    };
    report.write_history(&view, command.json)
}

#[cfg(test)]
mod tests {
    //! show の順序（読み出し → 範囲選択 → 集約 → severity 再算出 → 出力）と `--all` 絞り込みを mock で固定する。

    use super::run_show;
    use crate::update_history::domain::commands::ShowCommand;
    use crate::update_history::domain::wire::{
        ChangeCategory, ChangeItem, ChangeKind, PackageUpdate, Severity, UpdateEntry,
    };
    use crate::update_history::ports::{MockHistoryReportPort, MockHistoryStorePort};

    fn package(name: &str, declared: bool, category: ChangeCategory) -> PackageUpdate {
        PackageUpdate {
            name: name.to_string(),
            old: Some("1.0".to_string()),
            new: Some("1.1".to_string()),
            change: ChangeKind::Upgraded,
            declared,
            notes_url: None,
            change_items: vec![ChangeItem {
                category,
                text: "変更".to_string(),
                ref_url: None,
            }],
        }
    }

    fn entry(packages: Vec<PackageUpdate>) -> UpdateEntry {
        UpdateEntry {
            at: "2026-06-05T00:00:00Z".to_string(),
            nixpkgs_old: "a".to_string(),
            nixpkgs_new: "b".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::None,
            overall: String::new(),
            packages,
        }
    }

    fn command(all: bool) -> ShowCommand {
        ShowCommand {
            rev: None,
            limit: None,
            json: false,
            all,
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
}

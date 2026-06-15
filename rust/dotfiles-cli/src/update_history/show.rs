//! show / applied-summary use case: 履歴を読み、`state_old` 優先の catch-up 範囲を集約し、重要度連動ビューを
//! 出力する。
//!
//! 読み出し → 範囲選択（起点 state。旧履歴は `nixpkgs_old` fallback）→ catch-up 集約 → severity/overall 再算出
//! → 描画（text/JSON）の順に処理する。
//! 複数 bump を跨ぐ適用では複数エントリを跨ぐため、跨いだ全 [`UpdateEntry`] をアプリ単位で集約する: `old` は最古
//! 適用版・`new` は最新適用版、package 集約キーは `(name, source)`、各 package 内の change_item は決定論キー
//! `(category, ref_url, text)` で重複排除し、
//! severity / overall は集約後集合で record と同一規則（[`super::wire::severity_of`]）により再算出する（記録時と
//! 表示時で重要度規則を二重化しない）。
//!
//! 描画は injection 安全: `text` / `notes_url` / `ref_url` / `name` は LLM 抽出または上流ノート由来で信頼境界外で
//! あり、端末出力前に [`sanitize`] で ANSI escape・OSC・C0/C1 制御文字を除去する。JSON 出力（`--json`）は機械処理
//! 向けの生データ契約のため sanitize せず原値を保つ。

use std::collections::BTreeMap;
use std::path::Path;

use super::record::read_entries;
use super::wire::{
    CATEGORY_ORDER, ChangeItem, ChangeKind, PackageUpdate, Severity, UpdateEntry, category_emoji,
    overall_headline, severity_of,
};
use crate::Result;

// ---- 集約・範囲選択・ビュー ----

/// `show` 用に集約済みの履歴ビュー（表示意図の domain summary）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryView {
    /// 集約済みアプリ単位更新（安定順）。
    pub(crate) packages: Vec<PackageUpdate>,
    /// 集約後集合から再算出した全体重要度。
    pub(crate) severity: Severity,
    /// 集約後集合から再算出した機械見出し（例: `3アプリ更新: 🔒1 ✨2`）。
    pub(crate) overall: String,
}

/// catch-up 集約の同一性キー `(name, source)`。
type AggregateKey = (String, &'static str);

fn aggregate_key(package: &PackageUpdate) -> AggregateKey {
    (package.name.clone(), package.source.as_stable_key())
}

/// 集約後 `change` 種別を最古→最新の version 遷移から決める。
fn aggregated_change(first: ChangeKind, last: ChangeKind, spanned: bool) -> ChangeKind {
    match (first, last) {
        (_, ChangeKind::Removed) => ChangeKind::Removed,
        (ChangeKind::Added, _) => ChangeKind::Added,
        _ if spanned => ChangeKind::Upgraded,
        (single, _) => single,
    }
}

/// change_item の重複排除キー（category 安定キー・ref_url・text）。
type ItemDedupKey = (&'static str, Option<String>, String);

/// 1 アプリの集約途中状態（最初の change と span 有無、dedup 済み change_item の集合を保持する）。
#[derive(Clone)]
struct PackageAccum {
    package: PackageUpdate,
    first_change: ChangeKind,
    spanned: bool,
    seen_items: std::collections::BTreeSet<ItemDedupKey>,
}

/// 集約全体の途中状態（初出順 `order` と key→[`PackageAccum`] の `acc`）。
#[derive(Default)]
struct Aggregation {
    order: Vec<AggregateKey>,
    acc: BTreeMap<AggregateKey, PackageAccum>,
}

/// 複数 [`UpdateEntry`] を跨いだ更新をアプリ単位で集約し、安定順の [`PackageUpdate`] 列を返す。
fn aggregate(entries: &[UpdateEntry]) -> Vec<PackageUpdate> {
    let aggregation = entries
        .iter()
        .flat_map(|entry| entry.packages.iter())
        .fold(Aggregation::default(), fold_package);
    let acc = aggregation.acc;
    aggregation
        .order
        .into_iter()
        .filter_map(|key| acc.get(&key).cloned())
        .map(|accum| {
            let change = aggregated_change(accum.first_change, accum.package.change, accum.spanned);
            PackageUpdate {
                change,
                ..accum.package
            }
        })
        .collect()
}

/// 1 パッケージを集約状態へ畳み込む（初出は initial を作り、再出は new/change/notes_url/declared を更新する）。
fn fold_package(aggregation: Aggregation, package: &PackageUpdate) -> Aggregation {
    let Aggregation { order, acc } = aggregation;
    let key = aggregate_key(package);
    match acc.get(&key) {
        None => {
            let (change_items, seen_items) =
                merge_unique_items(Vec::new(), std::collections::BTreeSet::new(), package);
            let initial = PackageAccum {
                package: PackageUpdate {
                    name: package.name.clone(),
                    old: package.old.clone(),
                    new: package.new.clone(),
                    change: package.change,
                    declared: package.declared,
                    source: package.source,
                    notes_url: package.notes_url.clone(),
                    change_items,
                },
                first_change: package.change,
                spanned: false,
                seen_items,
            };
            Aggregation {
                order: order
                    .into_iter()
                    .chain(std::iter::once(key.clone()))
                    .collect(),
                acc: insert_accum(acc, key, initial),
            }
        }
        Some(existing) => {
            let (change_items, seen_items) = merge_unique_items(
                existing.package.change_items.clone(),
                existing.seen_items.clone(),
                package,
            );
            let merged = PackageAccum {
                package: PackageUpdate {
                    new: package.new.clone(),
                    change: package.change,
                    notes_url: package
                        .notes_url
                        .clone()
                        .or_else(|| existing.package.notes_url.clone()),
                    declared: existing.package.declared || package.declared,
                    change_items,
                    ..existing.package.clone()
                },
                first_change: existing.first_change,
                spanned: true,
                seen_items,
            };
            Aggregation {
                order,
                acc: insert_accum(acc, key, merged),
            }
        }
    }
}

/// `acc` に 1 件挿入した新しい map を返す（可変挿入を関数型の再構築へ閉じ込める）。
fn insert_accum(
    acc: BTreeMap<AggregateKey, PackageAccum>,
    key: AggregateKey,
    accum: PackageAccum,
) -> BTreeMap<AggregateKey, PackageAccum> {
    let mut acc = acc;
    acc.insert(key, accum);
    acc
}

/// 既存 change_item 列 + dedup 集合に、未出の change_item だけを順序を保って加えた新しい組を返す。
fn merge_unique_items(
    items: Vec<ChangeItem>,
    seen: std::collections::BTreeSet<ItemDedupKey>,
    package: &PackageUpdate,
) -> (Vec<ChangeItem>, std::collections::BTreeSet<ItemDedupKey>) {
    package
        .change_items
        .iter()
        .fold((items, seen), |(mut items, mut seen), item| {
            let dedup_key = (
                item.category.as_stable_key(),
                item.ref_url.clone(),
                item.text.clone(),
            );
            if seen.insert(dedup_key) {
                items.push(item.clone());
            }
            (items, seen)
        })
}

/// 表示対象エントリを起点 state（旧履歴は `nixpkgs_old` fallback）と件数上限で絞り込み、適用順
/// （最古→最新）の部分列を返す。
///
/// `state` が `Some` のとき、その key を `state_old`（旧履歴は `nixpkgs_old` fallback）に持つ最初の
/// エントリ以降を起点にする（一致無しは空）。
/// `None` なら全エントリ。`limit` は起点側（最古）から切る。
fn select_entries(
    entries: &[UpdateEntry],
    state: Option<&str>,
    limit: Option<usize>,
) -> Vec<UpdateEntry> {
    let start = match state {
        Some(state) => match entries
            .iter()
            .position(|entry| entry_state_old(entry) == state)
        {
            Some(index) => index,
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

fn entry_state_old(entry: &UpdateEntry) -> &str {
    entry.state_old.as_deref().unwrap_or(&entry.nixpkgs_old)
}

/// 選択済みエントリを catch-up 集約し、severity/overall を再算出した表示ビューを組み立てる。
///
/// `all=false` は宣言アプリ中心（既定）、`true` は低レベル依存も含める。update は常に全体適用のため出所で
/// 絞らない（全出所を表示）。severity/overall は絞り込み後集合に対し record と同一規則で再算出する。
fn build_view(selected: &[UpdateEntry], all: bool) -> HistoryView {
    let packages: Vec<PackageUpdate> = aggregate(selected)
        .into_iter()
        .filter(|package| all || package.declared)
        .collect();
    let all_items: Vec<ChangeItem> = packages
        .iter()
        .flat_map(|package| package.change_items.clone())
        .collect();
    let severity = severity_of(&all_items);
    let overall = overall_headline(packages.len(), &all_items);
    HistoryView {
        packages,
        severity,
        overall,
    }
}

// ---- 描画（text / JSON。injection 安全） ----

/// `json` 指定で生 JSON、未指定で重要度連動 text を組み立てる。
fn render(view: &HistoryView, json: bool) -> Result<String> {
    if json {
        render_json(view)
    } else {
        Ok(render_text(view))
    }
}

fn severity_badge(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical => "[critical] 🔒",
        Severity::Major => "[major] ⚠️",
        Severity::Minor => "[minor]",
        Severity::None => "[none]",
    }
}

/// 重要度連動の text 表示を組み立てる（全体見出し → severity バッジ → アプリ別 version + 変更項目）。
///
/// `packages` が空でも見出し（severity バッジ + overall）だけは必ず出す。
fn render_text(view: &HistoryView) -> String {
    let header = format!(
        "{} {}",
        severity_badge(view.severity),
        sanitize(&view.overall)
    );
    let package_lines = view.packages.iter().flat_map(|package| {
        // 見出し行に続けて、category 安定順に変更項目を並べる。
        let items = CATEGORY_ORDER.into_iter().flat_map(move |category| {
            package
                .change_items
                .iter()
                .filter(move |item| item.category == category)
                .map(render_change_item)
        });
        std::iter::once(render_package_heading(package)).chain(items)
    });
    std::iter::once(header)
        .chain(package_lines)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `name old → new`（不在側は `∅`）と任意の notes URL を 1 行で表す。version-only（change_items 空）は印を添える。
fn render_package_heading(package: &PackageUpdate) -> String {
    let name = sanitize(&package.name);
    let old = package
        .old
        .as_deref()
        .map(sanitize)
        .unwrap_or_else(|| "∅".to_string());
    let new = package
        .new
        .as_deref()
        .map(sanitize)
        .unwrap_or_else(|| "∅".to_string());
    // ノートが取れず version-only（version + notes_url のみ）で確定したパッケージは印を添える。概要付きは無印。
    let status_mark = if package.change_items.is_empty() {
        " [versionのみ]"
    } else {
        ""
    };
    match &package.notes_url {
        Some(url) => format!("  {name} {old} → {new} ({}){status_mark}", sanitize(url)),
        None => format!("  {name} {old} → {new}{status_mark}"),
    }
}

fn render_change_item(item: &ChangeItem) -> String {
    let emoji = category_emoji(item.category);
    let text = sanitize(&item.text);
    match &item.ref_url {
        Some(url) => format!("    {emoji} {text} ({})", sanitize(url)),
        None => format!("    {emoji} {text}"),
    }
}

/// 端末/ファイル出力前に untrusted 文字列から端末解釈される制御文字を除去する（tab 温存・改行は空白へ）。
fn sanitize(input: &str) -> String {
    input
        .chars()
        .filter_map(|ch| match ch {
            '\t' => Some('\t'),
            '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// 生データ（JSON）表現を組み立てる（`--json`。sanitize しない生データ契約）。
fn render_json(view: &HistoryView) -> Result<String> {
    #[derive(serde::Serialize)]
    struct JsonView<'a> {
        severity: &'a Severity,
        overall: &'a str,
        packages: &'a [PackageUpdate],
    }
    let dto = JsonView {
        severity: &view.severity,
        overall: &view.overall,
        packages: &view.packages,
    };
    Ok(serde_json::to_string_pretty(&dto)?)
}

// ---- use case ----

/// 利用者 `show`: 履歴 source を読み、起点 state（`state_old` 優先、旧履歴は `nixpkgs_old` fallback）からの
/// catch-up 区間を集約して stdout へ出力する。
pub(crate) fn run_show(
    source: &Path,
    rev: Option<&str>,
    limit: Option<usize>,
    json: bool,
    all: bool,
) -> Result<()> {
    let entries = read_entries(source)?;
    let selected = select_entries(&entries, rev, limit);
    let view = build_view(&selected, all);
    println!("{}", render(&view, json)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    //! catch-up 集約の old/new 確定・change_item 重複排除・安定順・範囲選択・`at` カーソル前進・描画（severity
    //! バッジ・category 順・injection 安全な制御文字除去・JSON）・use case の通し動作を固定する。

    use super::*;
    use crate::update_history::record::append_entry;
    use crate::update_history::wire::{ChangeCategory, PackageSource};

    fn entry_with_revs(
        at: &str,
        old: &str,
        new: &str,
        packages: Vec<PackageUpdate>,
    ) -> UpdateEntry {
        UpdateEntry {
            at: at.to_string(),
            state_old: None,
            state_new: None,
            nixpkgs_old: old.to_string(),
            nixpkgs_new: new.to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::None,
            overall: String::new(),
            packages,
        }
    }

    fn entry(at: &str, packages: Vec<PackageUpdate>) -> UpdateEntry {
        entry_with_revs(at, "old", "new", packages)
    }

    fn change_item(category: ChangeCategory, text: &str, ref_url: Option<&str>) -> ChangeItem {
        ChangeItem {
            category,
            text: text.to_string(),
            ref_url: ref_url.map(str::to_string),
        }
    }

    fn package_with_source(
        name: &str,
        old: Option<&str>,
        new: Option<&str>,
        change: ChangeKind,
        source: PackageSource,
        items: Vec<ChangeItem>,
    ) -> PackageUpdate {
        PackageUpdate {
            name: name.to_string(),
            old: old.map(str::to_string),
            new: new.map(str::to_string),
            change,
            declared: true,
            source,
            notes_url: None,
            change_items: items,
        }
    }

    fn package(
        name: &str,
        old: Option<&str>,
        new: Option<&str>,
        change: ChangeKind,
        items: Vec<ChangeItem>,
    ) -> PackageUpdate {
        package_with_source(name, old, new, change, PackageSource::Nix, items)
    }

    #[test]
    fn aggregate_takes_oldest_old_newest_new_and_dedups_items() {
        let entries = [
            entry(
                "2026-06-01T00:00:00Z",
                vec![package(
                    "neovim",
                    Some("0.10.0"),
                    Some("0.10.2"),
                    ChangeKind::Upgraded,
                    vec![change_item(
                        ChangeCategory::Fix,
                        "修正A",
                        Some("https://x/1"),
                    )],
                )],
            ),
            entry(
                "2026-06-02T00:00:00Z",
                vec![package(
                    "neovim",
                    Some("0.10.2"),
                    Some("0.11.0"),
                    ChangeKind::Upgraded,
                    vec![
                        change_item(ChangeCategory::Fix, "修正A", Some("https://x/1")),
                        change_item(ChangeCategory::Feature, "機能B", Some("https://x/2")),
                    ],
                )],
            ),
        ];
        let aggregated = aggregate(&entries);
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].old.as_deref(), Some("0.10.0"));
        assert_eq!(aggregated[0].new.as_deref(), Some("0.11.0"));
        // 同一決定論キーの「修正A」は重複排除され 2 件。
        assert_eq!(aggregated[0].change_items.len(), 2);
    }

    #[test]
    fn aggregate_preserves_multiple_refless_changes_in_same_category() {
        let entries = [entry(
            "2026-06-01T00:00:00Z",
            vec![package(
                "openssl",
                Some("3.0.0"),
                Some("3.0.1"),
                ChangeKind::Upgraded,
                vec![
                    change_item(ChangeCategory::Security, "CVE-A を修正", None),
                    change_item(ChangeCategory::Security, "CVE-B を修正", None),
                    change_item(ChangeCategory::Security, "CVE-C を修正", None),
                ],
            )],
        )];
        let aggregated = aggregate(&entries);
        assert_eq!(aggregated[0].change_items.len(), 3);
        assert_eq!(severity_of(&aggregated[0].change_items), Severity::Critical);
    }

    #[test]
    fn aggregate_keeps_same_name_nix_and_brew_separate() {
        let entries = [entry(
            "2026-06-01T00:00:00Z",
            vec![
                package_with_source(
                    "firefox",
                    Some("120"),
                    Some("121"),
                    ChangeKind::Upgraded,
                    PackageSource::Nix,
                    vec![change_item(ChangeCategory::Fix, "nix 側修正", None)],
                ),
                package_with_source(
                    "firefox",
                    Some("130"),
                    Some("131"),
                    ChangeKind::Upgraded,
                    PackageSource::Brew,
                    vec![change_item(ChangeCategory::Feature, "cask 側機能", None)],
                ),
            ],
        )];
        assert_eq!(aggregate(&entries).len(), 2);
    }

    #[test]
    fn aggregate_marks_removed_and_keeps_downgrade() {
        let removed = [
            entry(
                "2026-06-01T00:00:00Z",
                vec![package(
                    "oldpkg",
                    Some("1.0"),
                    Some("1.1"),
                    ChangeKind::Upgraded,
                    vec![],
                )],
            ),
            entry(
                "2026-06-02T00:00:00Z",
                vec![package(
                    "oldpkg",
                    Some("1.1"),
                    None,
                    ChangeKind::Removed,
                    vec![],
                )],
            ),
        ];
        let aggregated = aggregate(&removed);
        assert_eq!(aggregated[0].change, ChangeKind::Removed);
        assert_eq!(aggregated[0].new, None);
        assert_eq!(aggregated[0].old.as_deref(), Some("1.0"));

        let single = [entry(
            "2026-06-01T00:00:00Z",
            vec![package(
                "rolledback",
                Some("2.0"),
                Some("1.9"),
                ChangeKind::Downgraded,
                vec![],
            )],
        )];
        assert_eq!(aggregate(&single)[0].change, ChangeKind::Downgraded);
    }

    fn rev_entry(old: &str, new: &str) -> UpdateEntry {
        entry_with_revs(&format!("{old}->{new}"), old, new, Vec::new())
    }

    #[test]
    fn select_entries_resolves_start_and_limit() {
        let entries = [
            rev_entry("a", "b"),
            rev_entry("b", "c"),
            rev_entry("c", "d"),
        ];
        assert_eq!(select_entries(&entries, None, None).len(), 3);
        assert_eq!(select_entries(&entries, Some("b"), None).len(), 2);
        assert!(select_entries(&entries, Some("z"), None).is_empty());
        let limited = select_entries(&entries, None, Some(2));
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].nixpkgs_old, "a");
        assert!(select_entries(&entries, None, Some(0)).is_empty());
    }

    #[test]
    fn select_entries_prefers_state_old_for_tap_only_chain_links() {
        let mut first = rev_entry("nix-a", "nix-a");
        first.state_old = Some("lock-a".to_string());
        first.state_new = Some("lock-b".to_string());
        let mut second = rev_entry("nix-a", "nix-a");
        second.state_old = Some("lock-b".to_string());
        second.state_new = Some("lock-c".to_string());
        let entries = [first, second];

        let selected = select_entries(&entries, Some("lock-b"), None);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].state_old.as_deref(), Some("lock-b"));
        assert!(select_entries(&entries, Some("nix-a"), None).is_empty());
    }

    #[test]
    fn build_view_filters_declared_and_recomputes() {
        let declared = package(
            "neovim",
            Some("0.10"),
            Some("0.11"),
            ChangeKind::Upgraded,
            vec![change_item(ChangeCategory::Feature, "機能", None)],
        );
        let undeclared = PackageUpdate {
            declared: false,
            ..package(
                "libfoo",
                Some("1"),
                Some("2"),
                ChangeKind::Upgraded,
                vec![change_item(ChangeCategory::Fix, "修正", None)],
            )
        };
        let entries = [entry("2026-06-01T00:00:00Z", vec![declared, undeclared])];
        let default_view = build_view(&entries, false);
        assert_eq!(default_view.packages.len(), 1);
        assert_eq!(default_view.packages[0].name, "neovim");
        assert_eq!(default_view.severity, Severity::Minor);
        assert_eq!(build_view(&entries, true).packages.len(), 2);
    }

    fn view() -> HistoryView {
        HistoryView {
            packages: vec![PackageUpdate {
                name: "openssl".to_string(),
                old: Some("3.0.0".to_string()),
                new: Some("3.0.1".to_string()),
                change: ChangeKind::Upgraded,
                declared: true,
                source: PackageSource::Nix,
                notes_url: Some("https://github.com/openssl/openssl".to_string()),
                change_items: vec![
                    change_item(ChangeCategory::Feature, "新機能", None),
                    change_item(
                        ChangeCategory::Security,
                        "CVE 修正",
                        Some("https://github.com/openssl/openssl/pull/1"),
                    ),
                ],
            }],
            severity: Severity::Critical,
            overall: "1アプリ更新: 🔒1 ✨1".to_string(),
        }
    }

    #[test]
    fn text_lists_security_before_feature_with_badges() {
        let rendered = render_text(&view());
        let security_pos = rendered.find("CVE 修正");
        let feature_pos = rendered.find("新機能");
        assert!(security_pos.is_some() && feature_pos.is_some() && security_pos < feature_pos);
        assert!(rendered.contains("[critical] 🔒"));
        assert!(rendered.contains("🔒 CVE 修正"));
        assert!(rendered.contains("openssl 3.0.0 → 3.0.1"));
    }

    #[test]
    fn empty_view_emits_header_and_version_only_is_marked() {
        let empty = HistoryView {
            packages: Vec::new(),
            severity: Severity::None,
            overall: "0アプリ更新".to_string(),
        };
        assert_eq!(render_text(&empty), "[none] 0アプリ更新");

        // change_items 空（version-only）は `[versionのみ]` 印を添えて見せる。
        let HistoryView {
            packages,
            severity,
            overall,
        } = view();
        let v = HistoryView {
            packages: packages
                .into_iter()
                .map(|package| PackageUpdate {
                    change_items: Vec::new(),
                    ..package
                })
                .collect(),
            severity,
            overall,
        };
        assert!(render_text(&v).contains("[versionのみ]"));
    }

    #[test]
    fn text_strips_terminal_control_sequences() {
        let v = HistoryView {
            packages: vec![PackageUpdate {
                name: "pkg\u{1b}[2J".to_string(),
                old: Some("1.0".to_string()),
                new: Some("1.1".to_string()),
                change: ChangeKind::Upgraded,
                declared: true,
                source: PackageSource::Nix,
                notes_url: None,
                change_items: vec![change_item(
                    ChangeCategory::Security,
                    "悪意\u{1b}[31m赤\u{07}\n改行",
                    None,
                )],
            }],
            severity: Severity::Critical,
            overall: "見出し\u{9b}31m".to_string(),
        };
        let rendered = render_text(&v);
        assert!(!rendered.contains('\u{1b}'));
        assert!(!rendered.contains('\u{07}'));
        assert!(!rendered.contains('\u{9b}'));
        assert!(rendered.contains("赤 改行"));
        assert!(rendered.contains("pkg[2J 1.0 → 1.1"));
    }

    #[test]
    fn json_contains_severity_and_packages() -> Result<()> {
        let rendered = render_json(&view())?;
        assert!(rendered.contains("\"severity\": \"critical\""));
        assert!(rendered.contains("\"name\": \"openssl\""));
        Ok(())
    }

    fn show_package(name: &str, category: ChangeCategory) -> PackageUpdate {
        package(
            name,
            Some("1.0"),
            Some("1.1"),
            ChangeKind::Upgraded,
            vec![change_item(category, "変更", None)],
        )
    }

    fn write_dir(tag: &str, entries: &[UpdateEntry]) -> Result<std::path::PathBuf> {
        let dir =
            std::env::temp_dir().join(format!("dotfiles-uh-show-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        entries
            .iter()
            .try_for_each(|entry| append_entry(&dir.join("2026-06.toml"), entry))?;
        Ok(dir)
    }

    /// version-only パッケージ（ノートが取れず version + notes_url のみで確定。change_items 空）。
    fn version_only_package(name: &str) -> PackageUpdate {
        PackageUpdate {
            notes_url: Some("https://github.com/o/r/releases".to_string()),
            ..package(
                name,
                Some("1.0"),
                Some("1.1"),
                ChangeKind::Upgraded,
                Vec::new(),
            )
        }
    }

    #[test]
    fn version_only_heading_shows_label_version_and_notes_url() {
        // version-only は version + notes_url を示し、`[versionのみ]` 印を添える。概要付きは無印。
        let line = render_package_heading(&version_only_package("ghost"));
        assert!(line.contains("[versionのみ]"), "{line}");
        assert!(line.contains("ghost 1.0 → 1.1"), "{line}");
        assert!(line.contains("https://github.com/o/r/releases"), "{line}");

        let summarized = render_package_heading(&show_package("neovim", ChangeCategory::Feature));
        assert!(!summarized.contains("[versionのみ]"), "{summarized}");
    }

    #[test]
    fn run_show_reads_directory_without_error() -> Result<()> {
        let dir = write_dir(
            "run",
            &[entry_with_revs(
                "2026-06-01T00:00:00Z",
                "a",
                "b",
                vec![show_package("neovim", ChangeCategory::Feature)],
            )],
        )?;
        run_show(&dir, None, None, false, false)?;
        run_show(&dir, None, None, true, true)?;
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}

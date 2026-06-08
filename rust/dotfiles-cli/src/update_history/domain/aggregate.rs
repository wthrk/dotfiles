//! catch-up（複数 nightly bump 跨ぎ）の表示時集約を行う domain 関数。
//!
//! `last-applied-rev` が複数 bump 遅れていると、適用は複数エントリを一度に跨ぐ。`show` と適用後表示は
//! 跨いだ全 [`UpdateEntry`] をアプリ単位で集約する: `old` は最古適用版・`new` は最新適用版、
//! change_item は決定論キー `(name, category, ref_url, text)` で重複排除し、severity / overall は集約後集合で
//! 再算出する（再算出は [`super::severity`] の単一関数を共有）。集約はストアではなく表示時マージである。
//!
//! dedup キーに `text` を含める理由: 同一 app・同一 category・`ref_url` 無し（個別 URL を持たない複数の
//! security/fix 項目など）の異なる変更が複数あるとき、`text` を見ないと 2 件目以降が「同一」とみなされて
//! 落ちる。`text` を含めると本文が異なる変更は別物として保持され、本当に同一（全フィールド一致）の重複だけが
//! 排除される。決定論は保たれる（同一 entries 入力に対し常に同じ結果）。

use std::collections::BTreeMap;

use super::wire::{ChangeItem, ChangeKind, PackageUpdate, UpdateEntry};

/// catch-up 集約の同一性キー `(name, source)`。
///
/// `name` だけで畳むと、nix closure の `firefox` と cask の `firefox` が同じ catch-up span に入ったとき
/// 1 件に潰れ、old/new・declared・notes_url が後勝ちで誤表示される（別物の更新を 1 件として扱う）。出所
/// （[`super::wire::PackageSource`]）を同一性キーへ含め、同名でも nix/brew は別エントリとして集約する。`source`
/// の安定キーは `PackageSource::as_stable_key` を使い、`Debug` 表現に依存しない（dedup 決定論の根拠）。
type AggregateKey = (String, &'static str);

/// パッケージから集約キー `(name, source)` を作る。
fn aggregate_key(package: &PackageUpdate) -> AggregateKey {
    (package.name.clone(), package.source.as_stable_key())
}

/// 集約後 `change` 種別を最古→最新の version 遷移から決める。
///
/// 跨ぎ区間の途中種別は捨て、最初の適用前状態と最後の適用後状態だけで種別を確定する:
/// 開始時不在（最初が `Added`）かつ最後も削除でなければ `Added`、最後が `Removed` なら `Removed`、
/// それ以外は old→new の version 比較ではなく「上書き更新」を表す `Upgraded` を既定とし、
/// `Downgraded` は単一区間の種別が降格でかつ跨ぎが起きていないときに保持する。
fn aggregated_change(first: ChangeKind, last: ChangeKind, spanned: bool) -> ChangeKind {
    match (first, last) {
        (_, ChangeKind::Removed) => ChangeKind::Removed,
        (ChangeKind::Added, _) => ChangeKind::Added,
        _ if spanned => ChangeKind::Upgraded,
        (single, _) => single,
    }
}

/// 複数 [`UpdateEntry`] を跨いだ更新をアプリ単位で集約し、安定順の [`PackageUpdate`] 列を返す。
///
/// `entries` は適用順（`at` 昇順 = 最古→最新）で渡す。各アプリについて `old` は最初に現れたエントリの
/// `old`、`new` は最後に現れたエントリの `new` を採用する。`change_items` は出現順を保ったまま
/// 決定論キー `(name, category, ref_url, text)` で重複排除する（`text` を含める理由は本 module 先頭
/// コメント参照）。`notes_url` は最新エントリの値を優先する。
/// `declared` は跨ぎ区間で 1 度でも宣言アプリとして現れたら `true`（宣言表示を落とさない）。
///
/// 戻り値の並びは、最初に各アプリが現れた順を安定的に保つ。severity / overall の再算出は呼び出し側が
/// 集約結果の `change_items` を [`super::severity`] へ渡して行う（本関数は package 集約のみを担う）。
pub(crate) fn aggregate(entries: &[UpdateEntry]) -> Vec<PackageUpdate> {
    // `(name, source)` → 集約途中状態。挿入順を保つため別 Vec に出現順キーを記録する。
    let mut order: Vec<AggregateKey> = Vec::new();
    let mut acc: BTreeMap<AggregateKey, PackageUpdate> = BTreeMap::new();
    // change_item の重複排除キー `(name, source, category, ref_url, text)` の既出集合。
    // source/category 成分は `as_stable_key`（wire 文字列と一致する安定キー）を使い、`Debug` 表現に依存
    // しない（決定論の根拠）。source を含めるのは、同名でも nix/brew で別物の変更を誤って畳まないため。
    let mut seen: BTreeMap<(AggregateKey, &'static str, Option<String>, String), ()> =
        BTreeMap::new();
    // 各 `(name, source)` の最初の change 種別（集約 change 確定に使う）。
    let mut first_change: BTreeMap<AggregateKey, ChangeKind> = BTreeMap::new();
    // 各 `(name, source)` が複数エントリを跨いだか。
    let mut spanned: BTreeMap<AggregateKey, bool> = BTreeMap::new();

    for entry in entries {
        for package in &entry.packages {
            let key = aggregate_key(package);
            match acc.get_mut(&key) {
                None => {
                    order.push(key.clone());
                    first_change.insert(key.clone(), package.change);
                    spanned.insert(key.clone(), false);
                    let mut initial = PackageUpdate {
                        name: package.name.clone(),
                        old: package.old.clone(),
                        new: package.new.clone(),
                        change: package.change,
                        declared: package.declared,
                        source: package.source,
                        notes_url: package.notes_url.clone(),
                        change_items: Vec::new(),
                    };
                    push_unique_items(&mut seen, &key, &mut initial, &package.change_items);
                    acc.insert(key, initial);
                }
                Some(existing) => {
                    // new は最新エントリ、change/notes_url も最新を優先、declared は OR。
                    existing.new = package.new.clone();
                    existing.change = package.change;
                    if package.notes_url.is_some() {
                        existing.notes_url = package.notes_url.clone();
                    }
                    existing.declared = existing.declared || package.declared;
                    spanned.insert(key.clone(), true);
                    push_unique_items(&mut seen, &key, existing, &package.change_items);
                }
            }
        }
    }

    order
        .into_iter()
        .filter_map(|key| {
            let mut package = acc.remove(&key)?;
            let first = first_change.get(&key).copied().unwrap_or(package.change);
            let spanned = spanned.get(&key).copied().unwrap_or(false);
            package.change = aggregated_change(first, package.change, spanned);
            Some(package)
        })
        .collect()
}

/// 決定論キー `((name, source), category, ref_url, text)` の未出 change_item だけを順序を保って push する。
fn push_unique_items(
    seen: &mut BTreeMap<(AggregateKey, &'static str, Option<String>, String), ()>,
    key: &AggregateKey,
    package: &mut PackageUpdate,
    items: &[ChangeItem],
) {
    for item in items {
        let dedup_key = (
            key.clone(),
            // `Debug` 表現でなく wire 一致の安定キーを使い、dedup の決定論を保つ。
            item.category.as_stable_key(),
            item.ref_url.clone(),
            item.text.clone(),
        );
        if seen.insert(dedup_key, ()).is_none() {
            package.change_items.push(item.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    //! catch-up 集約の old/new 確定、change_item 重複排除、安定順を固定する。

    use super::*;
    use crate::update_history::domain::severity::{overall_headline, severity_of};
    use crate::update_history::domain::wire::{
        ChangeCategory, ChangeItem, ChangeKind, PackageSource, PackageUpdate, Severity, UpdateEntry,
    };

    fn entry(at: &str, packages: Vec<PackageUpdate>) -> UpdateEntry {
        UpdateEntry {
            at: at.to_string(),
            nixpkgs_old: "old".to_string(),
            nixpkgs_new: "new".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::None,
            overall: String::new(),
            packages,
        }
    }

    fn change_item(category: ChangeCategory, text: &str, ref_url: Option<&str>) -> ChangeItem {
        ChangeItem {
            category,
            text: text.to_string(),
            ref_url: ref_url.map(str::to_string),
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

    #[test]
    fn aggregate_takes_oldest_old_and_newest_new() {
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
                    vec![change_item(
                        ChangeCategory::Feature,
                        "機能B",
                        Some("https://x/2"),
                    )],
                )],
            ),
        ];

        let aggregated = aggregate(&entries);

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].old.as_deref(), Some("0.10.0"));
        assert_eq!(aggregated[0].new.as_deref(), Some("0.11.0"));
        assert_eq!(aggregated[0].change_items.len(), 2);
    }

    #[test]
    fn aggregate_dedups_only_fully_identical_change_items() {
        // dedup キーは `(name, category, ref_url, text)`。全フィールド一致の重複だけを排除し、
        // 同一 ref でも `text` が異なる変更は別物として保持する（個別 URL を共有する別内容を落とさない）。
        let item = change_item(ChangeCategory::Security, "CVE 修正", Some("https://x/cve"));
        let entries = [
            entry(
                "2026-06-01T00:00:00Z",
                vec![package(
                    "openssl",
                    Some("3.0.0"),
                    Some("3.0.1"),
                    ChangeKind::Upgraded,
                    // 完全同一の item を 2 度入れても 1 件に畳まれる（真の重複排除）。
                    vec![item.clone(), item.clone()],
                )],
            ),
            entry(
                "2026-06-02T00:00:00Z",
                vec![package(
                    "openssl",
                    Some("3.0.1"),
                    Some("3.0.2"),
                    ChangeKind::Upgraded,
                    // 同一 category/ref でも `text` が違えば別物として保持する。
                    vec![
                        change_item(
                            ChangeCategory::Security,
                            "別内容の同一参照",
                            Some("https://x/cve"),
                        ),
                        change_item(ChangeCategory::Feature, "新機能", None),
                    ],
                )],
            ),
        ];

        let aggregated = aggregate(&entries);

        assert_eq!(aggregated.len(), 1);
        // 完全同一 1 件 + 別 text の security 1 件 + feature 1 件 = 3 件。
        assert_eq!(aggregated[0].change_items.len(), 3);
        // 集約後集合で severity を再算出すると security により critical。
        assert_eq!(severity_of(&aggregated[0].change_items), Severity::Critical);
    }

    #[test]
    fn aggregate_preserves_multiple_refless_changes_in_same_category() {
        // P2-4 退行固定: 同 app・同 category・`ref_url` 無し（個別 URL を持たない複数の security 項目など）の
        // 異なる変更が、2 件目以降落ちずに全件保持されることを固定する。旧 dedup キー `(name, category, ref_url)`
        // は `text` を見ないため、これらを「同一」とみなして 1 件しか残さなかった。
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

        assert_eq!(aggregated.len(), 1);
        // 3 件すべて保持される（text で区別）。
        assert_eq!(aggregated[0].change_items.len(), 3);
        let texts: Vec<&str> = aggregated[0]
            .change_items
            .iter()
            .map(|item| item.text.as_str())
            .collect();
        assert_eq!(texts, vec!["CVE-A を修正", "CVE-B を修正", "CVE-C を修正"]);
        assert_eq!(severity_of(&aggregated[0].change_items), Severity::Critical);
    }

    #[test]
    fn aggregate_keeps_same_name_nix_and_brew_separate() {
        // 退行固定（P2: 出所込み集約キー）: 同名 `firefox` でも nix closure 由来と cask（brew）由来は別物の
        // 更新であり、catch-up span で 1 件に潰さず 2 件として保持する。旧実装は集約キーが `name` だけのため
        // old/new・notes_url が後勝ちで誤表示された。`(name, source)` をキーにして両者を区別する。
        let entries = [entry(
            "2026-06-01T00:00:00Z",
            vec![
                {
                    let mut p = package_with_source(
                        "firefox",
                        Some("120"),
                        Some("121"),
                        ChangeKind::Upgraded,
                        PackageSource::Nix,
                        vec![change_item(ChangeCategory::Fix, "nix 側修正", None)],
                    );
                    p.notes_url = Some("https://example.com/nix-firefox".to_string());
                    p
                },
                {
                    let mut p = package_with_source(
                        "firefox",
                        Some("130"),
                        Some("131"),
                        ChangeKind::Upgraded,
                        PackageSource::Brew,
                        vec![change_item(ChangeCategory::Feature, "cask 側機能", None)],
                    );
                    p.notes_url = Some("https://example.com/brew-firefox".to_string());
                    p
                },
            ],
        )];

        let aggregated = aggregate(&entries);

        // 1 件に潰れず、nix/brew の firefox が別エントリとして残る。
        assert_eq!(aggregated.len(), 2);
        let nix = aggregated
            .iter()
            .find(|p| p.source == PackageSource::Nix)
            .expect("nix firefox present");
        let brew = aggregated
            .iter()
            .find(|p| p.source == PackageSource::Brew)
            .expect("brew firefox present");
        // old/new・notes_url が後勝ちで混線せず、各出所の値を保つ。
        assert_eq!(nix.old.as_deref(), Some("120"));
        assert_eq!(nix.new.as_deref(), Some("121"));
        assert_eq!(
            nix.notes_url.as_deref(),
            Some("https://example.com/nix-firefox")
        );
        assert_eq!(brew.old.as_deref(), Some("130"));
        assert_eq!(brew.new.as_deref(), Some("131"));
        assert_eq!(
            brew.notes_url.as_deref(),
            Some("https://example.com/brew-firefox")
        );
        // change_item も出所ごとに別保持され、混ざらない。
        assert_eq!(nix.change_items.len(), 1);
        assert_eq!(nix.change_items[0].text, "nix 側修正");
        assert_eq!(brew.change_items.len(), 1);
        assert_eq!(brew.change_items[0].text, "cask 側機能");
    }

    #[test]
    fn aggregate_preserves_first_seen_app_order_and_recomputes_overall() {
        let entries = [entry(
            "2026-06-01T00:00:00Z",
            vec![
                package(
                    "zlib",
                    Some("1.2"),
                    Some("1.3"),
                    ChangeKind::Upgraded,
                    vec![change_item(ChangeCategory::Fix, "修正", None)],
                ),
                package(
                    "neovim",
                    Some("0.10"),
                    Some("0.11"),
                    ChangeKind::Upgraded,
                    vec![change_item(ChangeCategory::Feature, "機能", None)],
                ),
            ],
        )];

        let aggregated = aggregate(&entries);

        assert_eq!(aggregated[0].name, "zlib");
        assert_eq!(aggregated[1].name, "neovim");

        let all_items: Vec<ChangeItem> = aggregated
            .iter()
            .flat_map(|p| p.change_items.clone())
            .collect();
        assert_eq!(
            overall_headline(aggregated.len(), &all_items),
            "2アプリ更新: ✨1 🐛1"
        );
    }

    #[test]
    fn aggregate_marks_removed_when_last_span_removes_app() {
        let entries = [
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

        let aggregated = aggregate(&entries);

        assert_eq!(aggregated[0].change, ChangeKind::Removed);
        assert_eq!(aggregated[0].new, None);
        assert_eq!(aggregated[0].old.as_deref(), Some("1.0"));
    }

    #[test]
    fn stable_key_matches_wire_string_and_is_debug_independent() {
        // dedup キーの category 成分は `Debug` 派生表現でなく serde の wire 文字列（kebab-case）と一致する
        // 安定キーであることを固定する。`Debug` 表現（variant 名そのもの: 例 "DefaultChange"）に依存していれば
        // この assertion は失敗する。これにより将来 variant 名がリファクタで変わっても dedup キーは不変となる。
        let cases = [
            (ChangeCategory::Breaking, "breaking"),
            (ChangeCategory::Security, "security"),
            (ChangeCategory::Feature, "feature"),
            (ChangeCategory::Fix, "fix"),
            (ChangeCategory::Deprecation, "deprecation"),
            (ChangeCategory::DefaultChange, "default-change"),
        ];
        for (category, wire) in cases {
            assert_eq!(category.as_stable_key(), wire);
            // serde wire 文字列（TOML 値）と安定キーの一貫性を固定する。
            let rendered = toml::to_string(&ChangeItem {
                category,
                text: "x".to_string(),
                ref_url: None,
            })
            .expect("change_item serializes");
            assert!(
                rendered.contains(&format!("category = \"{wire}\"")),
                "wire 文字列 {wire} と安定キーが一致しない: {rendered}"
            );
            // `Debug` 表現（variant 名）に依存していないことを明示する。
            assert_ne!(
                category.as_stable_key(),
                format!("{category:?}"),
                "安定キーが Debug 表現に一致してはならない"
            );
        }
    }

    #[test]
    fn aggregate_preserves_downgraded_for_single_unspanned_entry() {
        // 単一区間（跨ぎ無し）で change=Downgraded のパッケージは、その種別をそのまま保持する
        // （spanned のときだけ Upgraded へ畳むため、単一区間では降格を捨てない）。
        let entries = [entry(
            "2026-06-01T00:00:00Z",
            vec![package(
                "rolledback",
                Some("2.0"),
                Some("1.9"),
                ChangeKind::Downgraded,
                vec![change_item(ChangeCategory::Fix, "差し戻し", None)],
            )],
        )];

        let aggregated = aggregate(&entries);

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].change, ChangeKind::Downgraded);
        assert_eq!(aggregated[0].old.as_deref(), Some("2.0"));
        assert_eq!(aggregated[0].new.as_deref(), Some("1.9"));
    }
}

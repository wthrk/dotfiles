//! 変更カテゴリ集合から重要度を機械算出する単一 domain 関数と、overall 機械見出し生成。
//!
//! severity は LLM 生成の自由文ではなく変更カテゴリ（閉集合 enum）からのみ決定論的に算出する。
//! これにより prompt injection に対して severity が改変されない。`record` と `show` は本 module の
//! 関数を共有し、算出規則を二重実装しない。

use super::wire::{ChangeCategory, ChangeItem, Severity};

/// 変更カテゴリ集合から重要度を機械算出する唯一の domain 関数。
///
/// 規則（決定論。`record` と `show` が共有）:
/// security を含む → `Critical`／breaking または deprecation（削除を含む）を含む → `Major`／
/// feature・fix・default-change のみ → `Minor`／空 → `None`。
/// severity は `text` などの自由文を一切見ず、`category` 閉集合だけを根拠にする。
pub(crate) fn severity_of(items: &[ChangeItem]) -> Severity {
    let mut has_major = false;
    let mut has_minor = false;
    for item in items {
        match item.category {
            ChangeCategory::Security => return Severity::Critical,
            ChangeCategory::Breaking | ChangeCategory::Deprecation => has_major = true,
            ChangeCategory::Feature | ChangeCategory::Fix | ChangeCategory::DefaultChange => {
                has_minor = true;
            }
        }
    }
    if has_major {
        Severity::Major
    } else if has_minor {
        Severity::Minor
    } else {
        Severity::None
    }
}

/// 各変更カテゴリの絵文字凡例（プラン確定の表示契約）。
///
/// 🔒security ⚠️breaking 🗑️deprecation/removal 🔧default-change ✨feature 🐛fix。
fn category_emoji(category: ChangeCategory) -> &'static str {
    match category {
        ChangeCategory::Security => "🔒",
        ChangeCategory::Breaking => "⚠️",
        ChangeCategory::Deprecation => "🗑️",
        ChangeCategory::DefaultChange => "🔧",
        ChangeCategory::Feature => "✨",
        ChangeCategory::Fix => "🐛",
    }
}

/// overall 機械見出しを生成する（例: `5アプリ更新: 🔒2 ⚠️1 ✨3`）。
///
/// `package_count` は更新アプリ数、`items` はそのエントリ全変更の集合。カテゴリ件数を
/// 凡例の絵文字付きで安定順（security→breaking→deprecation→default-change→feature→fix）に列挙する。
/// 件数 0 のカテゴリは省く。LLM 一行併記は任意であり本見出しには含めない（機械見出しのみ）。
pub(crate) fn overall_headline(package_count: usize, items: &[ChangeItem]) -> String {
    const ORDER: [ChangeCategory; 6] = [
        ChangeCategory::Security,
        ChangeCategory::Breaking,
        ChangeCategory::Deprecation,
        ChangeCategory::DefaultChange,
        ChangeCategory::Feature,
        ChangeCategory::Fix,
    ];
    let badges: Vec<String> = ORDER
        .iter()
        .filter_map(|category| {
            let count = items
                .iter()
                .filter(|item| item.category == *category)
                .count();
            (count > 0).then(|| format!("{}{count}", category_emoji(*category)))
        })
        .collect();
    if badges.is_empty() {
        format!("{package_count}アプリ更新")
    } else {
        format!("{package_count}アプリ更新: {}", badges.join(" "))
    }
}

#[cfg(test)]
mod tests {
    //! severity 機械算出と overall 見出しの決定論を固定する。

    use super::*;
    use crate::update_history::domain::wire::ChangeCategory;

    fn item(category: ChangeCategory) -> ChangeItem {
        ChangeItem {
            category,
            text: "変更".to_string(),
            ref_url: None,
        }
    }

    #[test]
    fn severity_security_dominates_to_critical() {
        let items = [item(ChangeCategory::Fix), item(ChangeCategory::Security)];
        assert_eq!(severity_of(&items), Severity::Critical);
    }

    #[test]
    fn severity_breaking_or_deprecation_is_major_without_security() {
        assert_eq!(
            severity_of(&[item(ChangeCategory::Breaking)]),
            Severity::Major
        );
        assert_eq!(
            severity_of(&[
                item(ChangeCategory::Deprecation),
                item(ChangeCategory::Feature)
            ]),
            Severity::Major
        );
    }

    #[test]
    fn severity_feature_fix_default_change_only_is_minor() {
        let items = [
            item(ChangeCategory::Feature),
            item(ChangeCategory::Fix),
            item(ChangeCategory::DefaultChange),
        ];
        assert_eq!(severity_of(&items), Severity::Minor);
    }

    #[test]
    fn severity_empty_is_none() {
        assert_eq!(severity_of(&[]), Severity::None);
    }

    #[test]
    fn overall_headline_lists_nonzero_categories_in_stable_order() {
        let items = [
            item(ChangeCategory::Security),
            item(ChangeCategory::Security),
            item(ChangeCategory::Breaking),
            item(ChangeCategory::Feature),
            item(ChangeCategory::Feature),
            item(ChangeCategory::Feature),
        ];
        assert_eq!(overall_headline(5, &items), "5アプリ更新: 🔒2 ⚠️1 ✨3");
    }

    #[test]
    fn overall_headline_without_change_items_reports_count_only() {
        assert_eq!(overall_headline(2, &[]), "2アプリ更新");
    }
}

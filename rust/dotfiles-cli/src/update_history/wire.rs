//! 更新履歴 TOML の wire/ドメイン型・閉集合 enum と、その型に閉じた純粋ドメイン規則（severity 機械算出・
//! LLM 出力/参照 URL のサニタイズ・SSRF fetch 許可ホスト判定）。
//!
//! field 名と enum 値は TOML スキーマ（`docs/update-history/<YYYY-MM>.toml`）に一致させる。`ref` は Rust
//! 予約語のため serde rename で TOML key `ref` に対応させる。閉集合（変更種別・変更カテゴリ・重要度）は生文字列
//! ではなく enum で表し、serde rename で TOML 値（kebab-case 含む）へ写す。
//!
//! severity は LLM 生成の自由文ではなく変更カテゴリ（閉集合 enum）からのみ決定論的に算出する（prompt injection
//! で severity が改変されない）。生リリースノートと LLM 出力は信頼境界外であり、TOML へ書く前に「許可ホストの
//! https URL だけを残す」「1 行概要の長さ・項目数を上限で切り詰める」で守る（[`sanitize_change_items`] /
//! [`is_allowed_url`] / SSRF の [`allowed_fetch_hosts`]）。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// 1 回の nightly bump で記録される更新エントリ（TOML `[[update]]` 1 件に対応）。
///
/// `at` はエントリ単位の RFC3339 タイムスタンプ。`severity` / `overall` はエントリ全体の重要度・機械見出しで、
/// いずれも `packages` の変更カテゴリから決定論的に算出される（[`severity_of`] 参照）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UpdateEntry {
    /// 適用時刻（RFC3339。CI が `--at` で注入する文字列をそのまま保持する）。
    pub(crate) at: String,
    /// bump 前の nixpkgs リビジョン。
    pub(crate) nixpkgs_old: String,
    /// bump 後の nixpkgs リビジョン。
    pub(crate) nixpkgs_new: String,
    /// diff 対象の参照構成（例: `darwinConfigurations.<ref>`）。
    pub(crate) reference: String,
    /// 変更カテゴリ集合から機械算出した全体重要度。
    pub(crate) severity: Severity,
    /// 「N アプリ更新: 🔒2 ⚠️1 ✨3」形式の機械見出し。
    pub(crate) overall: String,
    /// このエントリで更新された各パッケージ。
    #[serde(default, rename = "package")]
    pub(crate) packages: Vec<PackageUpdate>,
}

/// パッケージ更新の出所（nix closure か Homebrew cask か）。catch-up 集約の同一性キーの一部。
///
/// nix と brew は同名パッケージ（例 `firefox`）を別物として記録する。出所を同一性キーへ含めるため、wire にも
/// 残す。TOML 値は lowercase（`nix`/`brew`）。旧スキーマ（source 無し）の後方互換は `serde(default)` が担う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PackageSource {
    /// nix eval 由来（宣言パッケージの name→version 差分）。
    Nix,
    /// Homebrew tap 由来（cask/formula の版差分）。
    Brew,
}

impl Default for PackageSource {
    /// 旧スキーマ（source field を持たない既存 TOML）の deserialize 既定（保守的に `Nix`）。
    fn default() -> Self {
        PackageSource::Nix
    }
}

impl PackageSource {
    /// dedup・集約の決定論キーで使う安定文字列を返す（serde wire 文字列と一致）。
    pub(crate) fn as_stable_key(&self) -> &'static str {
        match self {
            PackageSource::Nix => "nix",
            PackageSource::Brew => "brew",
        }
    }
}

/// 1 アプリ/パッケージの version 差分と構造化変更リスト（TOML `[[update.package]]` に対応）。
///
/// `old` / `new` は `added` / `removed` で片側が `None` になりうるため `Option`。`declared` は宣言アプリかの区別、
/// `change_items` は LLM 抽出済みの構造化変更（取得不能/未抽出なら空）。`source` は出所（nix/brew）で集約の同一性
/// キーに含める。
///
/// ノートが取れたパッケージは `change_items`（概要）付き、取れないものは version-only（version old→new + notes_url
/// のみ、`change_items` 空）として 1 回の record で確定する。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PackageUpdate {
    /// パッケージ/アプリ名。catch-up 集約の同一性キーの一部（`source` と対）。
    pub(crate) name: String,
    /// 更新前 version（`added` では `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) old: Option<String>,
    /// 更新後 version（`removed` では `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) new: Option<String>,
    /// version 差分の種別。
    pub(crate) change: ChangeKind,
    /// 宣言アプリなら `true`（`show` 既定で表示）、低レベル依存なら `false`。
    pub(crate) declared: bool,
    /// 更新の出所（nix/brew）。旧スキーマ（source 無し）は `serde(default)` で `Nix` へ縮退する。
    #[serde(default)]
    pub(crate) source: PackageSource,
    /// リリースノート/changelog の URL（取得不能なら `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) notes_url: Option<String>,
    /// 構造化変更リスト（LLM 抽出。空なら version-only = 変更概要なし）。
    #[serde(default, rename = "change_item")]
    pub(crate) change_items: Vec<ChangeItem>,
}

/// 1 件の構造化変更（TOML `[[update.package.change_item]]` に対応）。
///
/// `category` は閉集合 enum で severity 算出の根拠になり、`text` は日本語 1 行の概要、`ref_url` はその変更の
/// PR/issue/release URL（任意）。catch-up 集約の重複排除は決定論キー `(name, category, ref_url, text)` で行う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChangeItem {
    /// 変更カテゴリ（severity 算出と表示グルーピングの根拠）。
    pub(crate) category: ChangeCategory,
    /// 簡潔な 1 行概要（日本語）。表示時はプレーン表示する契約（injection 耐性）。
    pub(crate) text: String,
    /// その変更の参照 URL。TOML key は予約語回避のため `ref`。
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub(crate) ref_url: Option<String>,
}

/// version 差分の種別（閉集合）。TOML 値は snake/lower 表現に一致させる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChangeKind {
    /// version が上がった。
    Upgraded,
    /// version が下がった。
    Downgraded,
    /// 新規追加された。
    Added,
    /// 削除された。
    Removed,
}

/// 構造化変更のカテゴリ（閉集合）。TOML 値は kebab-case（`default-change` 等）に一致させる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ChangeCategory {
    /// 破壊的変更。
    Breaking,
    /// セキュリティ修正。
    Security,
    /// 新機能。
    Feature,
    /// バグ修正。
    Fix,
    /// 非推奨化。
    Deprecation,
    /// デフォルト挙動変更。
    DefaultChange,
}

impl ChangeCategory {
    /// dedup・集約の決定論キーで使う安定文字列を返す（serde の wire 文字列＝TOML 値、kebab-case と一致）。
    ///
    /// `Debug` 派生表現に依存しないことが不変条件である（variant 名変更で dedup キーが変わらないようにする）。
    pub(crate) fn as_stable_key(&self) -> &'static str {
        match self {
            ChangeCategory::Breaking => "breaking",
            ChangeCategory::Security => "security",
            ChangeCategory::Feature => "feature",
            ChangeCategory::Fix => "fix",
            ChangeCategory::Deprecation => "deprecation",
            ChangeCategory::DefaultChange => "default-change",
        }
    }
}

/// エントリ全体の重要度（閉集合）。変更カテゴリ集合から機械算出する（[`severity_of`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Severity {
    /// security 変更を含む。
    Critical,
    /// breaking（破壊的変更）または deprecation（非推奨化）を含む。
    Major,
    /// 機能追加/修正のみ。
    Minor,
    /// 該当する変更がない。
    None,
}

// ---- severity 機械算出・overall 見出し ----

/// 変更カテゴリ集合から重要度を機械算出する関数。
///
/// 規則（決定論。`record` と `show` が共有）: security を含む → `Critical`／breaking または deprecation を含む →
/// `Major`／feature・fix・default-change のみ → `Minor`／空 → `None`。`text` 等の自由文は一切見ない。
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

/// 各変更カテゴリの絵文字凡例（表示契約）。🔒security ⚠️breaking 🗑️deprecation 🔧default-change ✨feature 🐛fix。
pub(crate) fn category_emoji(category: ChangeCategory) -> &'static str {
    match category {
        ChangeCategory::Security => "🔒",
        ChangeCategory::Breaking => "⚠️",
        ChangeCategory::Deprecation => "🗑️",
        ChangeCategory::DefaultChange => "🔧",
        ChangeCategory::Feature => "✨",
        ChangeCategory::Fix => "🐛",
    }
}

/// category 別グルーピング/ソートの安定順（破壊的・セキュリティを先頭に置く）。共有の表示/集計順。
pub(crate) const CATEGORY_ORDER: [ChangeCategory; 6] = [
    ChangeCategory::Security,
    ChangeCategory::Breaking,
    ChangeCategory::Deprecation,
    ChangeCategory::DefaultChange,
    ChangeCategory::Feature,
    ChangeCategory::Fix,
];

/// overall 機械見出しを生成する（例: `5アプリ更新: 🔒2 ⚠️1 ✨3`）。
///
/// `package_count` は更新アプリ数、`items` はそのエントリ全変更の集合。カテゴリ件数を凡例の絵文字付きで安定順
/// （security→breaking→deprecation→default-change→feature→fix）に列挙する。件数 0 のカテゴリは省く。
pub(crate) fn overall_headline(package_count: usize, items: &[ChangeItem]) -> String {
    let badges: Vec<String> = CATEGORY_ORDER
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

// ---- 参照 URL の host allowlist・SSRF fetch 許可ホスト・LLM 出力サニタイズ ----

/// エージェントの `fetch_url` に常に許可する GitHub 系ホスト集合（パッケージ毎集合の基底）。
const ALWAYS_ALLOWED_FETCH_HOSTS: [&str; 3] =
    ["github.com", "raw.githubusercontent.com", "api.github.com"];

/// 参照 URL に許可するホスト集合（https のみ。host 厳密一致）。
const ALLOWED_HOSTS: [&str; 4] = [
    "github.com",
    "gitlab.com",
    "raw.githubusercontent.com",
    "api.github.com",
];

/// 1 パッケージあたりに残す change_item の最大件数。
const MAX_ITEMS: usize = 12;

/// `text` 1 行概要の最大文字数（char 単位）。超過分は切り詰める（末尾に `…` を付すため最大 +1 文字）。
const MAX_TEXT_CHARS: usize = 200;

/// URL が許可ホストの https URL かを判定する（scheme 固定・credential 拒否・host case-insensitive）。
pub(crate) fn is_allowed_url(url: &str) -> bool {
    match host_of(url) {
        Some(host) => ALLOWED_HOSTS.iter().any(|allowed| host == *allowed),
        None => false,
    }
}

/// パッケージごとに `fetch_url` へ許可するホスト集合を eval メタのヒント（信頼境界内）から組み立てる。
///
/// ノート本文（信頼境界外）からは決して拡張しない（SSRF 防御の核）。常に github 系基底を含め、homepage/changelog
/// の https host を加える。返す集合は小文字化済み host の `BTreeSet`（決定論・重複排除）。
pub(crate) fn allowed_fetch_hosts(
    repo: Option<&str>,
    homepage: Option<&str>,
    changelog: Option<&str>,
) -> BTreeSet<String> {
    let mut hosts: BTreeSet<String> = ALWAYS_ALLOWED_FETCH_HOSTS
        .iter()
        .map(|host| host.to_string())
        .collect();
    let _ = repo;
    for hint in [homepage, changelog].into_iter().flatten() {
        if let Some(host) = host_of(hint) {
            hosts.insert(host);
        }
    }
    hosts
}

/// https URL から小文字化した host を抽出する純粋関数（credential 拒否・path injection 防御・SSRF 防御）。
///
/// 手組みの `split(':')` は IPv6（`[::1]`）やポート付き・credential 付き URL を正しく扱えず allowlist を
/// すり抜ける恐れがあるため、host 抽出は `url::Url` の厳密パースに委ねる。wire は純粋ドメイン層なので HTTP
/// クライアント型（`reqwest::Url`）ではなく既に `url::Host` を使う `url` crate へ直接寄せる。https 以外・
/// credential 付き・localhost / IP リテラル（ループバックや metadata service への到達源）は host を導かない。
pub(crate) fn host_of(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    // credential（`user:pass@`）付きは拒否する。
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    let host = parsed.host()?;
    match host {
        // IP リテラル（IPv4/IPv6）はループバック/メタデータサービス等への SSRF 源になるため拒否する。
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => None,
        url::Host::Domain(domain) => {
            let lowered = domain.to_ascii_lowercase();
            // `localhost` 等のローカル名はローカルサービス到達源になるため拒否する。
            if lowered.is_empty() || lowered == "localhost" {
                None
            } else {
                Some(lowered)
            }
        }
    }
}

/// エージェントが要求した URL が許可ホスト集合内の https かを判定する（SSRF 機械判定）。
pub(crate) fn fetch_host_allowed(url: &str, allowed_hosts: &BTreeSet<String>) -> bool {
    match host_of(url) {
        Some(host) => allowed_hosts.contains(&host),
        None => false,
    }
}

/// LLM 抽出済み change_item 列を記録前にサニタイズする（許可外 ref 破棄・空 text 除去・長さ/件数上限）。
pub(crate) fn sanitize_change_items(items: Vec<ChangeItem>) -> Vec<ChangeItem> {
    items
        .into_iter()
        .filter_map(|mut item| {
            let trimmed = item.text.trim();
            if trimmed.is_empty() {
                return None;
            }
            item.text = truncate_chars(trimmed, MAX_TEXT_CHARS);
            if item
                .ref_url
                .as_deref()
                .is_some_and(|url| !is_allowed_url(url))
            {
                item.ref_url = None;
            }
            Some(item)
        })
        .take(MAX_ITEMS)
        .collect()
}

/// 許可ホストの https でない `notes_url` を捨てる（`None` 化）。
pub(crate) fn sanitize_notes_url(notes_url: Option<String>) -> Option<String> {
    notes_url.filter(|url| is_allowed_url(url))
}

/// `text` を char 境界で最大長へ切り詰める。
fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut result: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    //! wire 型の TOML 直列化がスキーマ（field 名・enum 値・`ref` rename）に一致することをバイト固定する。

    use super::*;

    fn sample_entry() -> UpdateEntry {
        UpdateEntry {
            at: "2026-06-05T18:00:11Z".to_string(),
            nixpkgs_old: "a1b2c3d".to_string(),
            nixpkgs_new: "e4f5g6h".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::Critical,
            overall: "1アプリ更新: 🔒1 ✨1".to_string(),
            packages: vec![PackageUpdate {
                name: "neovim".to_string(),
                old: Some("0.10.2".to_string()),
                new: Some("0.11.0".to_string()),
                change: ChangeKind::Upgraded,
                declared: true,
                source: PackageSource::Nix,
                notes_url: Some(
                    "https://github.com/neovim/neovim/releases/tag/v0.11.0".to_string(),
                ),
                change_items: vec![
                    ChangeItem {
                        category: ChangeCategory::Security,
                        text: "セキュリティ修正".to_string(),
                        ref_url: Some("https://github.com/neovim/neovim/pull/1".to_string()),
                    },
                    ChangeItem {
                        category: ChangeCategory::Feature,
                        text: "新機能".to_string(),
                        ref_url: None,
                    },
                ],
            }],
        }
    }

    #[derive(Serialize)]
    struct HistoryDocument {
        #[serde(rename = "update")]
        updates: Vec<UpdateEntry>,
    }

    #[test]
    fn entry_serializes_to_plan_toml_schema() -> crate::Result<()> {
        let document = HistoryDocument {
            updates: vec![sample_entry()],
        };
        let rendered = toml::to_string(&document)?;
        let expected = "\
[[update]]
at = \"2026-06-05T18:00:11Z\"
nixpkgs_old = \"a1b2c3d\"
nixpkgs_new = \"e4f5g6h\"
reference = \"darwinConfigurations.ci\"
severity = \"critical\"
overall = \"1アプリ更新: 🔒1 ✨1\"

[[update.package]]
name = \"neovim\"
old = \"0.10.2\"
new = \"0.11.0\"
change = \"upgraded\"
declared = true
source = \"nix\"
notes_url = \"https://github.com/neovim/neovim/releases/tag/v0.11.0\"

[[update.package.change_item]]
category = \"security\"
text = \"セキュリティ修正\"
ref = \"https://github.com/neovim/neovim/pull/1\"

[[update.package.change_item]]
category = \"feature\"
text = \"新機能\"
";
        assert_eq!(rendered, expected);
        Ok(())
    }

    #[test]
    fn version_only_package_serializes_version_and_notes_url_with_no_change_item_table()
    -> crate::Result<()> {
        // version-only（change_items 空）は version + notes_url を書き、変更概要 table（`[[...change_item]]`）を
        // 持たない（空配列の round-trip で複元できる）。
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            #[serde(rename = "package")]
            packages: Vec<PackageUpdate>,
        }
        let pkg = PackageUpdate {
            name: "obscure".to_string(),
            old: Some("1.0".to_string()),
            new: Some("1.1".to_string()),
            change: ChangeKind::Upgraded,
            declared: true,
            source: PackageSource::Nix,
            notes_url: Some("https://github.com/o/r/releases".to_string()),
            change_items: Vec::new(),
        };
        let rendered = toml::to_string(&Wrap {
            packages: vec![pkg.clone()],
        })?;
        assert!(
            !rendered.contains("[[package.change_item]]"),
            "version-only は change_item table を書かない: {rendered}"
        );
        assert!(rendered.contains("notes_url = \"https://github.com/o/r/releases\""));
        let parsed: Wrap = toml::from_str(&rendered)?;
        assert_eq!(parsed.packages, vec![pkg]);
        Ok(())
    }

    #[test]
    fn package_without_optional_fields_parses() -> crate::Result<()> {
        // 後方互換: 旧スキーマ（source 無し）の `[[update.package]]` は Nix へ縮退して parse できる。
        let toml = "\
name = \"neovim\"
change = \"upgraded\"
declared = true
";
        let parsed: PackageUpdate = toml::from_str(toml)?;
        assert!(parsed.change_items.is_empty());
        assert_eq!(parsed.source, PackageSource::Nix);
        Ok(())
    }

    #[test]
    fn entry_round_trips_through_toml() -> crate::Result<()> {
        #[derive(Serialize, Deserialize)]
        struct Document {
            #[serde(rename = "update")]
            updates: Vec<UpdateEntry>,
        }
        let original = Document {
            updates: vec![sample_entry()],
        };
        let rendered = toml::to_string(&original)?;
        let parsed: Document = toml::from_str(&rendered)?;
        assert_eq!(parsed.updates, original.updates);
        Ok(())
    }

    #[test]
    fn closed_set_enums_round_trip() -> crate::Result<()> {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrap {
            change: ChangeKind,
            source: PackageSource,
        }
        let rendered = toml::to_string(&Wrap {
            change: ChangeKind::Downgraded,
            source: PackageSource::Brew,
        })?;
        assert!(rendered.contains("change = \"downgraded\""));
        assert!(rendered.contains("source = \"brew\""));
        let parsed: Wrap = toml::from_str(&rendered)?;
        assert_eq!(parsed.change, ChangeKind::Downgraded);
        assert_eq!(parsed.source, PackageSource::Brew);
        Ok(())
    }

    fn item(category: ChangeCategory) -> ChangeItem {
        ChangeItem {
            category,
            text: "変更".to_string(),
            ref_url: None,
        }
    }

    fn item_with(text: &str, ref_url: Option<&str>) -> ChangeItem {
        ChangeItem {
            category: ChangeCategory::Fix,
            text: text.to_string(),
            ref_url: ref_url.map(str::to_string),
        }
    }

    #[test]
    fn severity_is_mechanically_derived_from_categories() {
        // security 支配 → critical／breaking or deprecation → major／feature・fix・default-change → minor／空 → none。
        assert_eq!(
            severity_of(&[item(ChangeCategory::Fix), item(ChangeCategory::Security)]),
            Severity::Critical
        );
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
        assert_eq!(
            severity_of(&[
                item(ChangeCategory::Feature),
                item(ChangeCategory::Fix),
                item(ChangeCategory::DefaultChange)
            ]),
            Severity::Minor
        );
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
        assert_eq!(overall_headline(2, &[]), "2アプリ更新");
    }

    #[test]
    fn allows_only_https_allowlisted_hosts() {
        assert!(is_allowed_url("https://github.com/a/b/pull/1"));
        assert!(is_allowed_url("https://gitlab.com/a/b"));
        assert!(is_allowed_url(
            "https://api.github.com/repos/o/r/releases/tags/v1.2.3"
        ));
        assert!(is_allowed_url("https://API.GitHub.com/repos/o/r/releases"));
        assert!(is_allowed_url("https://RAW.githubusercontent.com/a/b"));
        assert!(!is_allowed_url("http://github.com/a/b"));
        assert!(!is_allowed_url("https://evil.example/github.com"));
        assert!(!is_allowed_url("https://api.github.com.evil.example/x"));
        assert!(!is_allowed_url("https://user@github.com/a"));
        assert!(!is_allowed_url("ftp://github.com/a"));
        assert!(!is_allowed_url("not a url"));
    }

    #[test]
    fn sanitize_change_items_drops_disallowed_ref_blank_text_and_caps() {
        let dropped =
            sanitize_change_items(vec![item_with("修正", Some("https://evil.example/x"))]);
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].ref_url, None);
        let kept = sanitize_change_items(vec![item_with(
            "修正",
            Some("https://github.com/a/b/pull/2"),
        )]);
        assert_eq!(
            kept[0].ref_url.as_deref(),
            Some("https://github.com/a/b/pull/2")
        );
        let blank = sanitize_change_items(vec![item_with("   ", None), item_with("実体", None)]);
        assert_eq!(blank.len(), 1);
        assert_eq!(blank[0].text, "実体");
        let long = "あ".repeat(MAX_TEXT_CHARS + 50);
        let truncated = sanitize_change_items(vec![item_with(&long, None)]);
        assert_eq!(truncated[0].text.chars().count(), MAX_TEXT_CHARS + 1);
        let many: Vec<ChangeItem> = (0..(MAX_ITEMS + 5))
            .map(|i| item_with(&format!("項目{i}"), None))
            .collect();
        assert_eq!(sanitize_change_items(many).len(), MAX_ITEMS);
    }

    #[test]
    fn sanitize_notes_url_filters_by_host() {
        assert_eq!(
            sanitize_notes_url(Some("https://github.com/a/b".to_string())).as_deref(),
            Some("https://github.com/a/b")
        );
        assert_eq!(
            sanitize_notes_url(Some("http://github.com/a".to_string())),
            None
        );
        assert_eq!(sanitize_notes_url(None), None);
    }

    #[test]
    fn allowed_fetch_hosts_basis_and_hints_only_from_trusted_meta() {
        let base = allowed_fetch_hosts(None, None, None);
        assert!(base.contains("github.com"));
        assert!(base.contains("raw.githubusercontent.com"));
        assert!(base.contains("api.github.com"));
        assert!(!base.contains("gitlab.com"));
        let hosts = allowed_fetch_hosts(
            Some("neovim/neovim"),
            Some("https://neovim.io/"),
            Some("https://gitlab.com/o/r/blob/v1/CHANGELOG.md"),
        );
        assert!(hosts.contains("neovim.io"));
        assert!(hosts.contains("gitlab.com"));
        // http / 不正ヒントは host を導かない。
        let ignored = allowed_fetch_hosts(None, Some("http://evil.example/x"), Some("not a url"));
        assert!(!ignored.contains("evil.example"));
        assert_eq!(ignored.len(), ALWAYS_ALLOWED_FETCH_HOSTS.len());
    }

    #[test]
    fn host_of_and_fetch_host_allowed() {
        assert_eq!(
            host_of("https://GitHub.com/a/b").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            host_of("https://neovim.io:443/").as_deref(),
            Some("neovim.io")
        );
        assert_eq!(host_of("http://github.com/a"), None);
        let hosts = allowed_fetch_hosts(None, Some("https://neovim.io/"), None);
        assert!(fetch_host_allowed("https://neovim.io/news", &hosts));
        assert!(fetch_host_allowed(
            "https://github.com/neovim/neovim/releases",
            &hosts
        ));
        assert!(!fetch_host_allowed("https://evil.example/x", &hosts));
        assert!(!fetch_host_allowed("http://neovim.io/x", &hosts));
    }

    #[test]
    fn host_of_rejects_ipv6_port_and_credential_via_url_parse() {
        // IPv6 リテラル（loopback 含む）は host を導かない（簡易 split パスのすり抜けを塞ぐ）。
        assert_eq!(host_of("https://[::1]/x"), None);
        assert_eq!(host_of("https://[2001:db8::1]:443/x"), None);
        // IPv4 リテラル（loopback / metadata service）も拒否する。
        assert_eq!(host_of("https://127.0.0.1/x"), None);
        assert_eq!(host_of("https://169.254.169.254/latest/meta-data"), None);
        // localhost も拒否する。
        assert_eq!(host_of("https://localhost/x"), None);
        // credential 付きは拒否する（`user@`・`user:pass@` の両形）。
        assert_eq!(host_of("https://user@github.com/a"), None);
        assert_eq!(host_of("https://user:pass@github.com/a"), None);
        // ポート付き許可ホストは host のみ（ポートを落として）正しく抽出する。
        assert_eq!(
            host_of("https://github.com:443/a/b").as_deref(),
            Some("github.com")
        );
        // allowlist 照合経路でも IPv6 / credential / localhost はすり抜けない。
        assert!(!is_allowed_url("https://[::1]/github.com"));
        assert!(!is_allowed_url("https://localhost/a"));
        assert!(!is_allowed_url("https://user:pass@github.com/a"));
        let hosts = allowed_fetch_hosts(None, None, None);
        assert!(!fetch_host_allowed("https://[::1]/x", &hosts));
        assert!(!fetch_host_allowed("https://169.254.169.254/x", &hosts));
    }
}

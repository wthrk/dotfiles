//! 更新履歴 TOML の wire/ドメイン型・閉集合 enum と、その型に閉じた純粋ドメイン規則（severity 機械算出・
//! LLM 出力/参照 URL のサニタイズ・SSRF の構造的 URL 判定）。
//!
//! field 名と enum 値は TOML スキーマ（`docs/update-history/<YYYY-MM>.toml`）に一致させる。`ref` は Rust
//! 予約語のため serde rename で TOML key `ref` に対応させる。閉集合（変更種別・変更カテゴリ・重要度）は生文字列
//! ではなく enum で表し、serde rename で TOML 値（kebab-case 含む）へ写す。
//!
//! severity は LLM 生成の自由文ではなく変更カテゴリ（閉集合 enum）からのみ決定論的に算出する（prompt injection
//! で severity が改変されない）。生リリースノートと LLM 出力は信頼境界外であり、TOML へ書く前に「構造的に安全な
//! https URL だけを残す」「1 行概要の長さ・項目数を上限で切り詰める」で守る（[`sanitize_change_items`] /
//! [`is_allowed_url`]）。
//!
//! **SSRF 防御は host_of の構造的検査で担保する**（狭いホスト allowlist では制限しない）。リリースノート/changelog
//! の所在はパッケージごとに異なり github に限らない（cargo は doc.rust-lang.org、iterm2 は iterm2.com 等）ため、
//! AI fetch も機械取得も「到達先ホスト一覧」での制限はしない。代わりに [`host_of`] が https 限定・credential 拒否・
//! IP リテラル（IPv4/IPv6）拒否・localhost 拒否・単一ラベルホスト（内部 DNS 名の疑い）拒否・既知の内部/メタデータ
//! host（`metadata.google.internal` 等のメタデータ FQDN・`.internal`/`.local`/`.localdomain`/`.localhost` で終わる
//! 内部 TLD）拒否を行い、これを SSRF の唯一の構造的境界にする。HTTP クライアント側の redirect 不追従・https 限定・サイズ/
//! 時間上限（[`super::notes`] の `build_http_client`）と合わせて防御する。

use serde::{Deserialize, Serialize};

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn parse_github_repo_url(url: &str) -> Option<(&str, &str, &str)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let (owner, after_owner) = rest.split_once('/')?;
    let owner = non_empty(Some(owner))?;
    let repo_raw = after_owner
        .split_once('/')
        .map_or(after_owner, |(repo, _)| repo);
    let repo_raw = non_empty(Some(repo_raw))?;
    let repo = repo_raw
        .split(['?', '#'])
        .next()
        .unwrap_or(repo_raw)
        .trim_end_matches(".git");
    let repo = non_empty(Some(repo))?;
    let tail = after_owner[repo_raw.len()..].trim_start_matches('/');
    Some((owner, repo, tail))
}

/// github URL 文字列から `owner/repo` を取り出す純粋関数（末尾 `.git`・クエリ/フラグメントは除く）。
pub(crate) fn repo_from_github_url(url: &str) -> Option<String> {
    parse_github_repo_url(url).map(|(owner, repo, _)| format!("{owner}/{repo}"))
}

/// GitHub release/tag/download URL から、版非依存な `.../releases` ヒント URL を導出する。
pub(crate) fn releases_url_from_github_url(url: &str) -> Option<String> {
    let (owner, repo, tail) = parse_github_repo_url(url)?;
    let tail = tail.split(['?', '#']).next().unwrap_or(tail);
    if tail == "releases"
        || tail == "releases/latest"
        || tail.starts_with("releases/tag/")
        || tail.starts_with("releases/latest/download/")
        || tail.starts_with("releases/download/")
    {
        Some(format!("https://github.com/{owner}/{repo}/releases"))
    } else {
        None
    }
}

/// 1 回の nightly bump で記録される更新エントリ（TOML `[[update]]` 1 件に対応）。
///
/// `at` はエントリ単位の RFC3339 タイムスタンプ。`severity` / `overall` はエントリ全体の重要度・機械見出しで、
/// いずれも `packages` の変更カテゴリから決定論的に算出される（[`severity_of`] 参照）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UpdateEntry {
    /// 適用時刻（RFC3339。CI が `--at` で注入する文字列をそのまま保持する）。
    pub(crate) at: String,
    /// bump 前 lock state key（`flake.lock` 内容ハッシュ。tap-only 更新でも一意な起点を持つ）。
    #[serde(default, alias = "cursor_old", skip_serializing_if = "Option::is_none")]
    pub(crate) state_old: Option<String>,
    /// bump 後 lock state key（`flake.lock` 内容ハッシュ）。
    #[serde(default, alias = "cursor_new", skip_serializing_if = "Option::is_none")]
    pub(crate) state_new: Option<String>,
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
/// 残す。TOML 値は lowercase（`nix`/`brew`）。source を持たない TOML は `serde(default)` で `Nix` へ縮退する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PackageSource {
    /// nix eval 由来（宣言パッケージの name→version 差分）。
    Nix,
    /// Homebrew tap 由来（cask/formula の版差分）。
    Brew,
}

impl Default for PackageSource {
    /// source field を持たない TOML の deserialize 既定（保守的に `Nix`）。
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
    /// 更新の出所（nix/brew）。source 省略時は `serde(default)` で `Nix` へ縮退する。
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
    if items
        .iter()
        .any(|item| item.category == ChangeCategory::Security)
    {
        return Severity::Critical;
    }
    let has_major = items.iter().any(|item| {
        matches!(
            item.category,
            ChangeCategory::Breaking | ChangeCategory::Deprecation
        )
    });
    let has_minor = items.iter().any(|item| {
        matches!(
            item.category,
            ChangeCategory::Feature | ChangeCategory::Fix | ChangeCategory::DefaultChange
        )
    });
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

// ---- 参照 URL の構造的 SSRF 判定・LLM 出力サニタイズ ----

/// 1 パッケージあたりに残す change_item の最大件数。
const MAX_ITEMS: usize = 12;

/// `text` 1 行概要の最大文字数（char 単位）。超過分は切り詰める（末尾に `…` を付すため最大 +1 文字）。
const MAX_TEXT_CHARS: usize = 200;

/// URL が構造的に安全な公開 https URL かを判定する SSRF の構造的フィルタ。
///
/// 狭いホスト allowlist には依存せず、[`host_of`] の構造的検査（https 限定・credential 拒否・IP リテラル拒否・
/// localhost / 単一ラベルホスト拒否・内部 DNS 名/メタデータ FQDN 拒否）が通れば許可する。リリースノートの所在は
/// github に限らない（cargo は doc.rust-lang.org、iterm2 は iterm2.com 等）ため、到達先ホストの一覧では制限しない。
/// 機械取得・AI fetch・provenance 学習・ref/notes_url サニタイズはこの 1 関数を共通の構造的境界として共有する。
///
/// この検査は URL 文字列の構造だけを見る。公開ドメインが private/link-local IP へ解決される DNS rebinding や、
/// 社内 DNS による接続先 IP ベースの SSRF は構造的検査では防げない。本機能は GitHub-hosted runner（内部
/// ネットワーク・社内 DNS 不在、メタデータは IP リテラルで拒否済み）専用であり、DNS rebinding は脅威モデルの
/// 対象外とする。実 IP 解決ガードは持たない。
pub(crate) fn is_allowed_url(url: &str) -> bool {
    host_of(url).is_some()
}

/// 内部 DNS / メタデータ FQDN を判定する内部 TLD 接尾辞（小文字・先頭ドット込みで照合する）。
///
/// `.internal`（GCP メタデータ FQDN `metadata.google.internal` を含む）・`.local`（mDNS）・`.localdomain`・
/// `.localhost` で終わるドット付きホストは内部名とみなして拒否する。公開ドメインはこれらの内部 TLD で終わらない。
const INTERNAL_TLD_SUFFIXES: [&str; 4] = [".internal", ".local", ".localdomain", ".localhost"];

/// ドット付きホストが内部 DNS 名/メタデータ FQDN かを判定する純粋関数。
///
/// 単一ラベルホストは [`host_of`] 側で既に拒否されるため、ここはドット付きの内部 TLD 接尾辞だけを見る。
fn is_internal_domain(host: &str) -> bool {
    INTERNAL_TLD_SUFFIXES
        .iter()
        .any(|suffix| host.ends_with(suffix))
}

/// https URL から小文字化した host を抽出する純粋関数（SSRF の構造的境界。credential 拒否・IP/localhost 拒否）。
///
/// 手組みの `split(':')` は IPv6（`[::1]`）やポート付き・credential 付き URL を正しく扱えず判定を
/// すり抜ける恐れがあるため、host 抽出は `url::Url` の厳密パースに委ねる。wire は純粋ドメイン層なので HTTP
/// クライアント型（`reqwest::Url`）ではなく既に `url::Host` を使う `url` crate へ直接寄せる。以下はいずれも
/// host を導かない（= 構造的に SSRF 候補を弾く）: https 以外・credential 付き・IP リテラル（IPv4/IPv6。10進/8進/
/// 16進/IDN 表記や IPv4-mapped IPv6 も `url` が IP へ正規化するため一律拒否。ループバックや metadata service への
/// 到達源）・localhost・単一ラベルホスト（`.` を含まない host。内部 DNS 名の疑い）・ドット付き内部 DNS 名/メタデータ
/// FQDN（`metadata.google.internal` や `.internal`/`.local`/`.localdomain`/`.localhost` で終わる host。
/// [`is_internal_domain`]）。判定前に末尾ドットを全て除いて DNS の絶対表記（`host.`）を相対表記と同一視し、
/// `localhost.`・`intranet.`・`metadata.google.internal.`（および `localhost..` のような多重末尾ドット）の
/// 末尾ドット付き内部名が判定をすり抜けるのを防ぐ。
/// DNS は実解決せず（hermetic）、構造的な接尾辞照合だけで判定する。公開ホストは TLD を持ち
/// 内部接尾辞で終わらないため許可する。文字列構造のみの検査のため、公開ドメインが private/link-local IP へ解決される
/// DNS rebinding や社内 DNS 経由の IP ベース SSRF は防げない（GitHub-hosted runner 専用前提で対象外）。
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
            // DNS の絶対表記（末尾ドット付き FQDN。例 `localhost.`・`metadata.google.internal.`）を相対表記と
            // 同一視するため、末尾ドットを全て除いて正規化してから localhost/単一ラベル/内部 DNS 判定にかける。
            // 正規化しないと `localhost.` が `== "localhost"` に不一致、`intranet.` が `.` を含み単一ラベル判定を
            // 回避、`metadata.google.internal.` が `.internal` 接尾辞に不一致となってすり抜ける。`url` crate は
            // 多重末尾ドット（`localhost..`）を保持するため、1 つだけ除く正規化では `localhost.` が残ってすり抜ける。
            let lowered_full = domain.to_ascii_lowercase();
            let lowered = lowered_full.trim_end_matches('.');
            // `localhost` 等のローカル名はローカルサービス到達源になるため拒否する。さらに単一ラベルホスト
            // （`.` を含まない host。例 `intranet`）は内部 DNS 名の可能性があるため拒否し、localhost 以外の
            // 内部名到達も塞ぐ。公開ドメインは必ず TLD を含み `.` を持つ。
            if lowered.is_empty() || lowered == "localhost" || !lowered.contains('.') {
                return None;
            }
            // ドット付き内部 DNS 名/メタデータ FQDN（`metadata.google.internal`・`*.internal`・`*.local` 等）も
            // クラウド内 metadata service への SSRF 源になるため拒否する。
            if is_internal_domain(lowered) {
                return None;
            }
            Some(lowered.to_string())
        }
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

/// `text` を char 境界で最大長へ切り詰める（超過時のみ末尾に `…` を付す）。
fn truncate_chars(text: &str, max_chars: usize) -> String {
    let head: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    //! wire 型の TOML 直列化がスキーマ（field 名・enum 値・`ref` rename）に一致することをバイト固定する。

    use super::*;

    fn sample_entry() -> UpdateEntry {
        UpdateEntry {
            at: "2026-06-05T18:00:11Z".to_string(),
            state_old: Some("lock-old".to_string()),
            state_new: Some("lock-new".to_string()),
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
    fn entry_serializes_to_plain_toml_schema() -> crate::Result<()> {
        let document = HistoryDocument {
            updates: vec![sample_entry()],
        };
        let rendered = toml::to_string(&document)?;
        let expected = "\
[[update]]
at = \"2026-06-05T18:00:11Z\"
state_old = \"lock-old\"
state_new = \"lock-new\"
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
        // source を持たない `[[update.package]]` は Nix へ縮退して parse できる。
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

    #[test]
    fn repo_from_github_url_strips_git_and_query() {
        assert_eq!(
            repo_from_github_url("https://github.com/o/r.git?x=1#frag").as_deref(),
            Some("o/r")
        );
        assert_eq!(
            repo_from_github_url("http://github.com/o/r/issues/1").as_deref(),
            Some("o/r")
        );
        assert_eq!(repo_from_github_url("https://gitlab.com/o/r"), None);
        assert_eq!(repo_from_github_url("https://github.com/o"), None);
    }

    #[test]
    fn releases_url_from_github_url_normalizes_release_variants_only() {
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/releases").as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/releases?after=v1.2.3").as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/releases#latest").as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/releases/tag/v1.2.3").as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/releases/latest").as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/releases/latest?after=v1")
                .as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/releases/latest#asset").as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            releases_url_from_github_url(
                "https://github.com/o/r/releases/latest/download/x86_64-apple-darwin.tar.gz"
            )
            .as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/releases/download/v1/x.zip")
                .as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(releases_url_from_github_url("https://github.com/o/r"), None);
        assert_eq!(
            releases_url_from_github_url("https://github.com/o/r/issues/1"),
            None
        );
    }

    #[test]
    fn update_entry_reads_legacy_cursor_fields_into_state_fields() -> Result<(), toml::de::Error> {
        let entry: UpdateEntry = toml::from_str(
            r#"
at = "2026-06-13T09:45:17Z"
cursor_old = "lock-old"
cursor_new = "lock-new"
nixpkgs_old = "old"
nixpkgs_new = "new"
reference = "darwinConfigurations.ci-ref"
severity = "none"
overall = "0アプリ更新"
"#,
        )?;
        assert_eq!(entry.state_old.as_deref(), Some("lock-old"));
        assert_eq!(entry.state_new.as_deref(), Some("lock-new"));
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
    fn allows_structurally_safe_public_https_beyond_github() {
        // github 系に限らず、構造的に安全な公開 https を許可する（ノート所在は github 外にもある）。
        assert!(is_allowed_url("https://github.com/a/b/pull/1"));
        assert!(is_allowed_url("https://gitlab.com/a/b"));
        assert!(is_allowed_url(
            "https://api.github.com/repos/o/r/releases/tags/v1.2.3"
        ));
        assert!(is_allowed_url("https://API.GitHub.com/repos/o/r/releases"));
        assert!(is_allowed_url("https://RAW.githubusercontent.com/a/b"));
        // cargo / iterm2 等、github 外のリリースノート所在も許可する。
        assert!(is_allowed_url(
            "https://doc.rust-lang.org/nightly/cargo/CHANGELOG.html"
        ));
        assert!(is_allowed_url("https://iterm2.com/downloads.html"));
        assert!(is_allowed_url("https://example.com/changelog"));
        // SSRF 構造的拒否は維持・強化する。
        assert!(!is_allowed_url("http://github.com/a/b"));
        assert!(!is_allowed_url("https://user@github.com/a"));
        assert!(!is_allowed_url("https://user:pass@github.com/a"));
        assert!(!is_allowed_url("ftp://github.com/a"));
        assert!(!is_allowed_url("not a url"));
        // IP リテラル・localhost・単一ラベルホスト（内部 DNS 名）は拒否する。
        assert!(!is_allowed_url("https://127.0.0.1/x"));
        assert!(!is_allowed_url("https://169.254.169.254/latest/meta-data"));
        assert!(!is_allowed_url("https://[::1]/x"));
        assert!(!is_allowed_url("https://localhost/a"));
        assert!(!is_allowed_url("https://intranet/a"));
        // ドット付き内部 DNS 名/メタデータ FQDN（クラウド内 metadata service への SSRF 源）も拒否する。
        assert!(!is_allowed_url(
            "https://metadata.google.internal/computeMetadata/v1/"
        ));
        assert!(!is_allowed_url("https://service.internal/a"));
        assert!(!is_allowed_url("https://printer.local/a"));
        assert!(!is_allowed_url("https://host.localdomain/a"));
        assert!(!is_allowed_url("https://foo.localhost/a"));
        // 内部 TLD を接尾辞に含むが公開 TLD で終わる host（偽装）は許可する。
        assert!(is_allowed_url("https://internal.example.com/a"));
        assert!(is_allowed_url("https://metadata.example.com/a"));
        // 末尾ドット付き FQDN（DNS 絶対表記）の内部名は末尾ドット正規化後に拒否する。
        assert!(!is_allowed_url("https://localhost./"));
        assert!(!is_allowed_url("https://intranet./"));
        assert!(!is_allowed_url(
            "https://metadata.google.internal./computeMetadata/v1/"
        ));
        // 多重末尾ドット（`url` crate が保持する）も全て除く正規化で内部名として拒否する。
        assert!(!is_allowed_url("https://localhost../"));
        assert!(!is_allowed_url("https://localhost.../"));
        assert!(!is_allowed_url(
            "https://metadata.google.internal../computeMetadata/v1/"
        ));
        // 末尾ドット付きでも公開ドメインは正規化後に許可する。
        assert!(is_allowed_url("https://github.com./a/b"));
    }

    #[test]
    fn sanitize_change_items_drops_disallowed_ref_blank_text_and_caps() {
        // 構造的に安全でない ref（http / localhost）は破棄する。公開 https の ref は残す（github 外でも可）。
        let dropped = sanitize_change_items(vec![item_with("修正", Some("http://example.com/x"))]);
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].ref_url, None);
        let dropped_local =
            sanitize_change_items(vec![item_with("修正", Some("https://localhost/x"))]);
        assert_eq!(dropped_local[0].ref_url, None);
        let kept = sanitize_change_items(vec![item_with(
            "修正",
            Some("https://github.com/a/b/pull/2"),
        )]);
        assert_eq!(
            kept[0].ref_url.as_deref(),
            Some("https://github.com/a/b/pull/2")
        );
        // github 外の公開 https ref も表示用途として許容する。
        let kept_off_github = sanitize_change_items(vec![item_with(
            "修正",
            Some("https://doc.rust-lang.org/cargo/CHANGELOG.html"),
        )]);
        assert_eq!(
            kept_off_github[0].ref_url.as_deref(),
            Some("https://doc.rust-lang.org/cargo/CHANGELOG.html")
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
    fn host_of_extracts_public_hosts_and_rejects_single_label() {
        assert_eq!(
            host_of("https://GitHub.com/a/b").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            host_of("https://neovim.io:443/").as_deref(),
            Some("neovim.io")
        );
        // github 外の公開ドメインも host を導く（cargo / iterm2）。
        assert_eq!(
            host_of("https://doc.rust-lang.org/cargo/CHANGELOG.html").as_deref(),
            Some("doc.rust-lang.org")
        );
        assert_eq!(
            host_of("https://iterm2.com/downloads.html").as_deref(),
            Some("iterm2.com")
        );
        assert_eq!(host_of("http://github.com/a"), None);
        // 単一ラベルホスト（内部 DNS 名の疑い。`.` を含まない）は拒否する。
        assert_eq!(host_of("https://intranet/a"), None);
        assert_eq!(host_of("https://internal-service/a"), None);
    }

    #[test]
    fn host_of_rejects_internal_dns_and_metadata_fqdn_but_allows_public_lookalikes() {
        // ドット付き内部 DNS 名/メタデータ FQDN（DNS は実解決せず接尾辞照合で構造的に拒否する）。
        assert_eq!(
            host_of("https://metadata.google.internal/computeMetadata/v1/"),
            None
        );
        assert_eq!(host_of("https://service.internal/a"), None);
        assert_eq!(host_of("https://printer.local/a"), None);
        assert_eq!(host_of("https://host.localdomain/a"), None);
        assert_eq!(host_of("https://foo.localhost/a"), None);
        // 公開 TLD で終わる host は内部 TLD ラベルを含んでも許可する（接尾辞照合なので誤検知しない）。
        assert_eq!(
            host_of("https://internal.example.com/a").as_deref(),
            Some("internal.example.com")
        );
        assert_eq!(
            host_of("https://metadata.example.com/a").as_deref(),
            Some("metadata.example.com")
        );
        assert_eq!(
            host_of("https://localhost.example.com/a").as_deref(),
            Some("localhost.example.com")
        );
        // 末尾ドット付き FQDN（DNS 絶対表記）は正規化後に内部名判定されるため拒否する。
        assert_eq!(host_of("https://localhost./"), None);
        assert_eq!(host_of("https://intranet./"), None);
        assert_eq!(
            host_of("https://metadata.google.internal./computeMetadata/v1/"),
            None
        );
        assert_eq!(host_of("https://service.internal./a"), None);
        // 多重末尾ドット（`url` crate が保持する）は全て除いて正規化するため内部名判定で拒否する。
        assert_eq!(host_of("https://localhost../"), None);
        assert_eq!(host_of("https://localhost.../"), None);
        assert_eq!(
            host_of("https://metadata.google.internal../computeMetadata/v1/"),
            None
        );
        // 末尾ドット付きでも公開ドメインは正規化後に許可する（`github.com.` → `github.com`）。
        assert_eq!(
            host_of("https://github.com./a/b").as_deref(),
            Some("github.com")
        );
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
        // ポート付きホストは host のみ（ポートを落として）正しく抽出する。
        assert_eq!(
            host_of("https://github.com:443/a/b").as_deref(),
            Some("github.com")
        );
        // is_allowed_url（構造的境界）でも IPv6 / credential / localhost / 単一ラベルはすり抜けない。
        assert!(!is_allowed_url("https://[::1]/github.com"));
        assert!(!is_allowed_url("https://localhost/a"));
        assert!(!is_allowed_url("https://user:pass@github.com/a"));
        assert!(!is_allowed_url("https://169.254.169.254/x"));
        assert!(!is_allowed_url("https://intranet/x"));
    }
}

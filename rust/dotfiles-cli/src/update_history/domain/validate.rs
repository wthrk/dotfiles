//! LLM 抽出結果と参照 URL を記録前に機械バリデートする純粋 domain 規則。
//!
//! 生リリースノートと LLM 出力は信頼境界外（prompt injection 源）であり、TOML へ書き込む前に
//! 「許可ホストの https URL だけを残す」「1 行概要の長さ・項目数を上限で切り詰める」という業務規則で
//! 守る。enum 値の妥当性は wire 型の deserialize で既に閉じているため、ここでは host / 長さ / 件数の
//! 機械判定だけを domain rule として固定する。severity は別途 category enum から算出するため本 module では扱わない。

use std::collections::BTreeSet;

use super::wire::ChangeItem;

/// エージェントの `fetch_url` ツールに常に許可する GitHub 系ホスト集合。
///
/// パッケージごとの fetch 許可ホスト集合（[`allowed_fetch_hosts`]）の基底になる。リリースノートは
/// GitHub Releases ページ・API・raw ファイルに置かれることが多く、これらの公式 host はパッケージに依らず
/// 常に許可してよい（取得・記録 URL の host allowlist [`ALLOWED_HOSTS`] とも整合する）。`gitlab.com` は
/// 一部パッケージのノート置き場だが「常時許可」ではなくパッケージの宣言 host（homepage/repo）に
/// gitlab.com が現れたときだけ [`allowed_fetch_hosts`] が個別に加える。
const ALWAYS_ALLOWED_FETCH_HOSTS: [&str; 3] =
    ["github.com", "raw.githubusercontent.com", "api.github.com"];

/// 参照 URL に許可するホスト集合（https のみ）。
///
/// `ref` / `notes_url` はこの集合のいずれかを host に持つ https URL だけを残す。攻撃者が生ノートへ
/// 埋め込んだ任意 URL を記録・表示経路へ通さないための allowlist であり、prefix でなく host の厳密一致で判定する。
///
/// `api.github.com` は GitHub 公式 REST API（リリースノート本文 `.body` 取得）の host である。notes adapter は
/// `github.com/.../releases/tag/<tag>` を Releases API（`api.github.com/repos/.../releases/tags/<tag>`）へ変換して
/// 取得するため、その変換後 URL が allowlist を通る必要がある。公式 API のみを host 厳密一致で許可し、`.body`
/// 本文は依然信頼境界外（prompt injection 源）として後段の機械バリデートで守る。
const ALLOWED_HOSTS: [&str; 4] = [
    "github.com",
    "gitlab.com",
    "raw.githubusercontent.com",
    "api.github.com",
];

/// 1 パッケージあたりに残す change_item の最大件数。
const MAX_ITEMS: usize = 12;

/// `text` 1 行概要の最大文字数（char 単位）。超過分は切り詰める。
const MAX_TEXT_CHARS: usize = 200;

/// URL が許可ホストの https URL かを判定する。
///
/// `https://<host>[:port]/...` 形式で、`<host>` が [`ALLOWED_HOSTS`] のいずれかに一致する場合だけ
/// `true`。host は RFC 上 case-insensitive なため、allowlist との一致は ASCII 大文字小文字を無視して
/// 判定する（`https://GitHub.com/...` 等の正当な URL を弾かない）。scheme が https でない、host が
/// allowlist 外、形式不正はすべて `false`。scheme 固定・credential（`@`）拒否・path injection 防御
/// （host を最初の `/` までで切る）は維持する。記録・表示双方が共有する。
pub(crate) fn is_allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    // host 部分は最初の `/` までを取り、credential（`@`）は許可しない。
    let authority = rest.split('/').next().unwrap_or("");
    if authority.contains('@') || authority.is_empty() {
        return false;
    }
    let host = authority.split(':').next().unwrap_or("");
    // host は case-insensitive（RFC）なので allowlist 一致も大文字小文字を無視する。
    ALLOWED_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

/// パッケージごとに `fetch_url` ツールへ許可するホスト集合を eval メタのヒントから組み立てる（domain rule）。
///
/// **信頼境界**: 許可ホスト集合は eval（`eval-declared-versions.sh`、信頼境界内）由来の値だけから組み立てる。
/// `repo`（GitHub `owner/repo`）・`homepage`・`changelog` はすべて評価時属性であり、攻撃者が制御するノート
/// 本文（信頼境界外）からは決して拡張しない。これが SSRF 防御の核である: エージェント（LLM）はノート本文に
/// 現れた任意 URL を fetch 要求しうるが、その host がこの集合外なら adapter は fetch せず「not allowed」を返す。
///
/// 集合の構成:
/// - 常に [`ALWAYS_ALLOWED_FETCH_HOSTS`]（github.com / raw.githubusercontent.com / api.github.com）。
///   リリースノートの一次取得元（Releases ページ・API・raw changelog）であり、パッケージに依らず公式。
/// - `repo` が `owner/repo` 形なら、その forge host として `github.com`（既に基底に含む）。`repo` は eval が
///   GitHub owner/repo として抽出した値なので追加 host は不要だが、将来 forge 拡張時の拡張点として host 抽出
///   経路を通す。
/// - `homepage` / `changelog` の URL から host を抽出して加える（例: `neovim.io`、`gitlab.com`）。これにより
///   エージェントは「そのパッケージの正規ドメイン」のノートを自分で辿れる。host 抽出は https URL に限る
///   （[`host_of`]）。
///
/// 返す集合は小文字化済み host の `BTreeSet`（決定論・重複排除）。adapter はこの集合と [`fetch_host_allowed`]
/// で fetch 可否を機械判定する。
pub(crate) fn allowed_fetch_hosts(
    repo: Option<&str>,
    homepage: Option<&str>,
    changelog: Option<&str>,
) -> BTreeSet<String> {
    let mut hosts: BTreeSet<String> = ALWAYS_ALLOWED_FETCH_HOSTS
        .iter()
        .map(|host| host.to_string())
        .collect();
    // repo（owner/repo）は GitHub 由来（eval が github URL から抽出）なので host は github.com（基底に含む）。
    // owner/repo に `/` 以外の host 情報は無いため、ここでは追加 host を導かない（将来 forge 拡張の通り道）。
    let _ = repo;
    for hint in [homepage, changelog].into_iter().flatten() {
        if let Some(host) = host_of(hint) {
            hosts.insert(host);
        }
    }
    hosts
}

/// https URL から小文字化した host を抽出する純粋関数（credential 拒否・path injection 防御）。
///
/// `https://<host>[:port]/...` の `<host>` を小文字で返す。scheme が https でない、credential（`@`）を含む、
/// host が空の URL は `None`。[`is_allowed_url`] と同じ host 切り出し規則を使い、抽出 host を allowlist 構築や
/// 機械判定の単一の host 解釈源にする。
pub(crate) fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split('/').next().unwrap_or("");
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let host = authority.split(':').next().unwrap_or("");
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// エージェントが要求した `fetch_url` の URL が、そのパッケージの許可ホスト集合に属する https かを判定する。
///
/// `allowed_hosts` は [`allowed_fetch_hosts`] が eval メタから組み立てた小文字化 host 集合。URL は https で
/// かつその host が集合に含まれるときだけ `true`。scheme 不正・credential 含み・host 不一致は `false`。
/// ノート本文（信頼境界外）から得た URL でも、この機械判定を必ず通すことで SSRF（許可外 host への横滑り）を塞ぐ。
pub(crate) fn fetch_host_allowed(url: &str, allowed_hosts: &BTreeSet<String>) -> bool {
    match host_of(url) {
        Some(host) => allowed_hosts.contains(&host),
        None => false,
    }
}

/// LLM 抽出済み change_item 列を記録前にサニタイズする。
///
/// 規則（決定論。`record` が記録前に適用する）:
/// - `ref` が `Some` でも許可ホストの https でなければ URL を捨てる（`None` 化）。category/text は残す。
/// - `text` が空白だけの項目は捨てる（根拠不明な概要を記録しない）。
/// - `text` は前後空白を除き、[`MAX_TEXT_CHARS`] 文字を超える分を切り詰める。
/// - 残った項目は出現順を保ち、先頭から [`MAX_ITEMS`] 件までに制限する。
///
/// enum 妥当性は deserialize 済み [`ChangeItem`] で保証されるため、本関数は host / 長さ / 件数だけを扱う。
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
///
/// 取得 adapter が返した URL であっても、記録前に host allowlist を機械適用して injection 経路を塞ぐ。
pub(crate) fn sanitize_notes_url(notes_url: Option<String>) -> Option<String> {
    notes_url.filter(|url| is_allowed_url(url))
}

/// `text` を char 境界で最大長へ切り詰める。
fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut result: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        // 切り詰めたことを示し、途中で切れた概要だと分かるようにする。
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    //! host allowlist 判定と change_item サニタイズ（URL 破棄・空 text 除去・件数/長さ上限）を固定する。

    use super::*;
    use crate::update_history::domain::wire::{ChangeCategory, ChangeItem};

    fn item(text: &str, ref_url: Option<&str>) -> ChangeItem {
        ChangeItem {
            category: ChangeCategory::Fix,
            text: text.to_string(),
            ref_url: ref_url.map(str::to_string),
        }
    }

    #[test]
    fn allows_only_https_allowlisted_hosts() {
        assert!(is_allowed_url("https://github.com/a/b/pull/1"));
        assert!(is_allowed_url("https://gitlab.com/a/b"));
        assert!(!is_allowed_url("http://github.com/a/b"));
        assert!(!is_allowed_url("https://evil.example/github.com"));
        assert!(!is_allowed_url("https://user@github.com/a"));
        assert!(!is_allowed_url("ftp://github.com/a"));
        assert!(!is_allowed_url("not a url"));
    }

    #[test]
    fn allows_api_github_com_for_releases_api() {
        // releases/tag → Releases API 変換の取得先 host を allowlist に追加した退行固定。
        // 公式 API host は許可し、他は依然拒否（紛らわしい近傍 host も弾く）。
        assert!(is_allowed_url(
            "https://api.github.com/repos/o/r/releases/tags/v1.2.3"
        ));
        assert!(is_allowed_url("https://API.GitHub.com/repos/o/r/releases"));
        assert!(!is_allowed_url("https://api.github.com.evil.example/x"));
        assert!(!is_allowed_url("https://evil-api.github.com/x"));
        assert!(!is_allowed_url("http://api.github.com/repos/o/r"));
    }

    #[test]
    fn allows_mixed_case_allowlisted_hosts() {
        // host は RFC 上 case-insensitive。allowlist との一致は大文字小文字を無視するため、
        // `GitHub.com` / `RAW.githubusercontent.com` のような大小混在の正当な host も許可する。
        assert!(is_allowed_url("https://GitHub.com/a/b/pull/1"));
        assert!(is_allowed_url("https://GITHUB.COM/a/b"));
        assert!(is_allowed_url("https://RAW.githubusercontent.com/a/b"));
        assert!(is_allowed_url("https://GitLab.com/a/b"));
        // allowlist 外は大小を変えても依然拒否（case-insensitive 化が allowlist を緩めない）。
        assert!(!is_allowed_url("https://EVIL.example/github.com"));
        assert!(!is_allowed_url("https://NotGithub.com/a"));
    }

    #[test]
    fn sanitize_drops_disallowed_ref_but_keeps_item() {
        let items = sanitize_change_items(vec![item("修正", Some("https://evil.example/x"))]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].ref_url, None);
        assert_eq!(items[0].text, "修正");
    }

    #[test]
    fn sanitize_keeps_allowlisted_ref() {
        let items =
            sanitize_change_items(vec![item("修正", Some("https://github.com/a/b/pull/2"))]);
        assert_eq!(
            items[0].ref_url.as_deref(),
            Some("https://github.com/a/b/pull/2")
        );
    }

    #[test]
    fn sanitize_drops_blank_text_items() {
        let items = sanitize_change_items(vec![item("   ", None), item("実体", None)]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "実体");
    }

    #[test]
    fn sanitize_truncates_long_text_and_caps_item_count() {
        let long = "あ".repeat(MAX_TEXT_CHARS + 50);
        let truncated = sanitize_change_items(vec![item(&long, None)]);
        assert_eq!(truncated[0].text.chars().count(), MAX_TEXT_CHARS + 1); // +1 for the ellipsis

        let many: Vec<ChangeItem> = (0..(MAX_ITEMS + 5))
            .map(|i| item(&format!("項目{i}"), None))
            .collect();
        assert_eq!(sanitize_change_items(many).len(), MAX_ITEMS);
    }

    #[test]
    fn allowed_fetch_hosts_always_includes_github_family() {
        // 退行固定（SSRF allowlist 基底）: ヒントが何も無くても github 系公式 host は常に許可される。
        let hosts = allowed_fetch_hosts(None, None, None);
        assert!(hosts.contains("github.com"));
        assert!(hosts.contains("raw.githubusercontent.com"));
        assert!(hosts.contains("api.github.com"));
        // gitlab.com は「常時許可」ではない（宣言 host に現れたときだけ加わる）。
        assert!(!hosts.contains("gitlab.com"));
    }

    #[test]
    fn allowed_fetch_hosts_adds_homepage_and_changelog_hosts() {
        // 退行固定: homepage / changelog の host を eval メタから集合へ加える（パッケージの正規ドメイン）。
        let hosts = allowed_fetch_hosts(
            Some("neovim/neovim"),
            Some("https://neovim.io/"),
            Some("https://gitlab.com/o/r/blob/v1/CHANGELOG.md"),
        );
        assert!(hosts.contains("neovim.io"));
        assert!(hosts.contains("gitlab.com"));
        // 基底 host も維持。
        assert!(hosts.contains("github.com"));
    }

    #[test]
    fn allowed_fetch_hosts_ignores_non_https_hints() {
        // http / 不正 URL のヒントは host を導かない（https のみ）。基底 host は残る。
        let hosts = allowed_fetch_hosts(None, Some("http://evil.example/x"), Some("not a url"));
        assert!(!hosts.contains("evil.example"));
        assert_eq!(hosts.len(), ALWAYS_ALLOWED_FETCH_HOSTS.len());
    }

    #[test]
    fn host_of_extracts_lowercased_host_for_https_only() {
        assert_eq!(
            host_of("https://GitHub.com/a/b").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            host_of("https://neovim.io:443/").as_deref(),
            Some("neovim.io")
        );
        assert_eq!(host_of("http://github.com/a"), None);
        assert_eq!(host_of("https://user@github.com/a"), None);
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn fetch_host_allowed_matches_set_membership_for_https() {
        // 退行固定（SSRF 機械判定）: 集合内 host の https のみ許可。集合外・非 https は拒否する。
        let hosts = allowed_fetch_hosts(None, Some("https://neovim.io/"), None);
        assert!(fetch_host_allowed("https://neovim.io/news", &hosts));
        assert!(fetch_host_allowed(
            "https://github.com/neovim/neovim/releases",
            &hosts
        ));
        // ノート本文に現れた許可外 host は拒否（横滑り防止）。
        assert!(!fetch_host_allowed("https://evil.example/x", &hosts));
        // https でなければ拒否。
        assert!(!fetch_host_allowed("http://neovim.io/x", &hosts));
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
}

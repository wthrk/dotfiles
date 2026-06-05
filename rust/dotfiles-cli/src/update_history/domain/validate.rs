//! LLM 抽出結果と参照 URL を記録前に機械バリデートする純粋 domain 規則。
//!
//! 生リリースノートと LLM 出力は信頼境界外（prompt injection 源）であり、TOML へ書き込む前に
//! 「許可ホストの https URL だけを残す」「1 行概要の長さ・項目数を上限で切り詰める」という業務規則で
//! 守る。enum 値の妥当性は wire 型の deserialize で既に閉じているため、ここでは host / 長さ / 件数の
//! 機械判定だけを domain rule として固定する。severity は別途 category enum から算出するため本 module では扱わない。

use super::wire::ChangeItem;

/// 参照 URL に許可するホスト集合（https のみ）。
///
/// `ref` / `notes_url` はこの集合のいずれかを host に持つ https URL だけを残す。攻撃者が生ノートへ
/// 埋め込んだ任意 URL を記録・表示経路へ通さないための allowlist であり、prefix でなく host の厳密一致で判定する。
const ALLOWED_HOSTS: [&str; 3] = ["github.com", "gitlab.com", "raw.githubusercontent.com"];

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

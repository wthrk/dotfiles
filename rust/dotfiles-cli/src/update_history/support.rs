//! `update-history` の process-generic な安全 fetch primitive（業務語彙を持たない技術境界）。
//!
//! リリースノート取得元（notes adapter / GitHub Models tool-use ループ）は、いずれも信頼境界外 URL を
//! curl で取得する。その curl は **redirect 不追従**（`-L` 無し・`--max-redirs 0`）・**https 限定**
//! （`--proto =https`）でなければならない。redirect を追従すると、初期 URL の host だけ検査した allowlist を
//! 越えて 3xx 応答経由で許可外 host から本文を取得しうる（`--proto =https` は scheme 制限であって host 制限
//! ではない）。この安全 curl 経路を複数 adapter で二重実装すると、片方だけ `-L` を足すような退行が起きうる
//! ため、process-generic な安全 fetch primitive としてここへ一本化し、notes adapter と github_models adapter の
//! 双方から再利用する。
//!
//! 本 module は **業務語彙を持たない**: 「どの URL が許可されるか」「どこからノートを取得するか」「provenance を
//! どう学習するか」といった判断は持たない。host 集合の SSRF 判定は domain
//! （[`fetch_host_allowed`](crate::update_history::domain::validate::fetch_host_allowed)）が担い、本 module は
//! 「与えられた許可 host 集合に属する https URL だけを安全 curl で取得する」という技術境界だけを担う。
//! 取得テキストの truncate（上限文字数）や `RawReleaseNotes` への翻訳は呼び出し側 adapter の責務とする。

use std::collections::BTreeSet;
use std::ffi::OsString;

use crate::Result;
use crate::process::run_capture;
use crate::update_history::domain::validate::fetch_host_allowed;

/// 安全 fetch（redirect 不追従・https 限定）の curl 引数列を組み立てる純粋関数。
///
/// **redirect を追従しない**こと（`--location` を含めず `--max-redirs 0`）が host allowlist 契約の要であり、
/// 引数列をテストで固定して退行を防ぐ。`-L` 無しのため curl は初期 URL の host 以外を踏まず、これが allowlist
/// 契約（allowlist 外へ踏まない）を保証する。`--fail` は 4xx/5xx を失敗にする（3xx は `--fail` では失敗にならず、
/// `-L` 無しのため追従もされず body 無しの 3xx として空縮退する）。`--proto =https` で https 以外の scheme を拒む。
pub(in crate::update_history) fn safe_fetch_args(url: &str) -> [OsString; 8] {
    [
        OsString::from("--fail"),
        OsString::from("--silent"),
        OsString::from("--show-error"),
        // redirect を追従しない（allowlist 外 host への横滑りを塞ぐ）。`-L` 無しのため 3xx は追従されず
        // body 無しで返り、空本文として `None` へ縮退する（`--fail` は 4xx/5xx 対象で 3xx は失敗にしない）。
        // `--max-redirs 0` は明示的に「リダイレクトを 0 回まで」とする補強。
        OsString::from("--max-redirs"),
        OsString::from("0"),
        OsString::from("--proto"),
        OsString::from("=https"),
        OsString::from(url),
    ]
}

/// 与えた https URL を redirect 不追従・https 限定の curl で取得し、非空本文だけを `Some` で返す。
///
/// host allowlist 検査は呼び出し側（domain 判定）が済ませている前提の **技術 primitive** であり、ここでは
/// host を再判定しない（host 集合判定は domain の責務、本 module は安全 curl だけを担う）。取得失敗
/// （ネットワーク不通・404・`--fail` 由来の非 0 終了）と空本文はいずれも `Ok(None)`（呼び出し側で graceful
/// degradation）。redirect は追従しない（[`safe_fetch_args`] 参照）ため初期 host 以外を踏まない。
pub(in crate::update_history) fn safe_https_fetch(url: &str) -> Result<Option<String>> {
    match run_capture("curl", safe_fetch_args(url)) {
        Ok(text) if !text.trim().is_empty() => Ok(Some(text)),
        // 空本文または取得失敗はノート無しとして縮退する。
        Ok(_) | Err(_) => Ok(None),
    }
}

/// 許可 host 集合に属する https URL だけを安全 curl で取得する（host 集合判定 + 安全 fetch の合成）。
///
/// AI エージェント（GitHub Models tool-use ループ）や registry 再利用が要求した URL を、SSRF 検査つきで
/// 取得するための合成 primitive である。`allowed_hosts` は呼び出し側が eval メタ（信頼境界内）のヒント host
/// だけから組み立てたパッケージごとの許可 host 集合で、host 集合の所属判定は domain の
/// [`fetch_host_allowed`] が機械判定する（**業務 host 集合の構築は呼び出し側、所属判定は domain、安全 curl は
/// 本 support**、と責務を分ける）。URL の host が集合外、または https 以外なら **fetch せず** `Ok(None)`
/// （呼び出し側はツール結果として「not allowed」を返す）。集合内 https のみ実際に curl を起動する。ノート本文
/// （信頼境界外）から拾った URL でも、この機械判定を必ず通すことで許可外 host への横滑りを塞ぐ。取得失敗・
/// 空本文も `Ok(None)`。返す本文の truncate（上限文字数）は呼び出し側 adapter の責務（adapter ごとの上限）。
pub(in crate::update_history) fn fetch_allowed_note(
    url: &str,
    allowed_hosts: &BTreeSet<String>,
) -> Result<Option<String>> {
    if !fetch_host_allowed(url, allowed_hosts) {
        return Ok(None);
    }
    safe_https_fetch(url)
}

#[cfg(test)]
mod tests {
    //! 安全 fetch 引数列の redirect 不追従・https 限定（host allowlist 契約をコードで保証）と、
    //! `fetch_allowed_note` の host 集合判定（許可外 host は curl を起動せず None）を hermetic に固定する。

    use super::{fetch_allowed_note, safe_fetch_args};
    use crate::update_history::domain::validate::allowed_fetch_hosts;

    #[test]
    fn safe_fetch_args_do_not_follow_redirects() {
        // 退行固定: redirect を追従すると host allowlist 契約に違反する。`--location` を含めず
        // `--max-redirs 0`・`--proto =https` を引数列で固定する。
        let args: Vec<String> = safe_fetch_args("https://github.com/a/b")
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            !args.iter().any(|arg| arg == "--location" || arg == "-L"),
            "redirect を追従してはならない: {args:?}"
        );
        let max_redirs_idx = args
            .iter()
            .position(|arg| arg == "--max-redirs")
            .expect("--max-redirs を指定する");
        assert_eq!(args.get(max_redirs_idx + 1).map(String::as_str), Some("0"));
        let proto_idx = args
            .iter()
            .position(|arg| arg == "--proto")
            .expect("--proto を指定する");
        assert_eq!(args.get(proto_idx + 1).map(String::as_str), Some("=https"));
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://github.com/a/b")
        );
    }

    #[test]
    fn fetch_allowed_note_skips_disallowed_host_without_running_curl() -> crate::Result<()> {
        // 退行固定（SSRF）: 許可 host 集合外の URL は domain 判定で弾かれ、curl を一切起動せず None を返す
        // （hermetic: network 非依存）。許可 host 集合は eval メタ由来のヒントだけから組み立てる。
        let allowed = allowed_fetch_hosts(Some("neovim/neovim"), None, None);
        // ノート本文由来の許可外 host は fetch せず None（curl を起動しない）。
        assert!(fetch_allowed_note("https://evil.example/x", &allowed)?.is_none());
        // https 以外も host 判定前に弾かれる（curl を起動しない）。
        assert!(fetch_allowed_note("http://github.com/a/b", &allowed)?.is_none());
        Ok(())
    }
}

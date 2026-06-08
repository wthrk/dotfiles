//! `update-history` の process-generic な安全 fetch primitive（業務語彙を持たない技術境界）。
//!
//! リリースノート取得元（notes adapter / GitHub Models tool-use ループ）は、いずれも信頼境界外 URL を
//! curl で取得する。その curl は **redirect 不追従**（`-L` 無し・`--max-redirs 0`）・**https 限定**
//! （`--proto =https`）でなければならない。redirect を追従すると、初期 URL の host だけ検査した allowlist を
//! 越えて 3xx 応答経由で許可外 host から本文を取得しうる（`--proto =https` は scheme 制限であって host 制限
//! ではない）。`-L` を付けない限り curl は 3xx の `Location` を **追従しない**ため、本文を返すのは常に初期 URL の
//! host（= 既に allowlist 検査済みの host）に限られ、許可外 host へは横滑りしない。なお 3xx 応答でも本文が
//! 返ること自体はあり得る（サーバ実装次第）が、それは許可済み host からの本文であり、内容は後段の機械
//! バリデート（host/長さ/件数）で扱う。この安全 curl 経路を複数 adapter で二重実装すると、片方だけ `-L` を
//! 足すような退行が起きうるため、process-generic な安全 fetch primitive としてここへ一本化し、notes adapter と
//! github_models adapter の双方から再利用する。
//!
//! 本 module は **業務語彙を持たない**かつ **他層の業務語彙へ依存しない**（言語標準ライブラリと外部技術 crate、
//! および同 crate の process primitive にのみ依存する）。「どの URL が許可されるか」「どこからノートを取得するか」
//! 「provenance をどう学習するか」といった判断は持たず、host 集合の SSRF 判定（domain）と本 module の安全 curl の
//! **合成**も持たない。本 module が担うのは「与えられた https URL を redirect 不追従・https 限定・有界 timeout/サイズ
//! で取得する」という純技術境界だけであり、host 集合判定（domain）と安全 fetch（この support）の組み立ては
//! 呼び出し側 adapter（notes / github_models）の責務とする。取得テキストの truncate（上限文字数）や
//! `RawReleaseNotes` への翻訳も呼び出し側 adapter の責務とする。

use std::ffi::OsString;

use crate::Result;
use crate::process::run_capture;

/// 安全 fetch の接続上限秒（`--connect-timeout`）。許可 host が接続を受けない/遅延する場合に有界で打ち切る。
const CONNECT_TIMEOUT_SECS: &str = "10";
/// 安全 fetch の転送全体の上限秒（`--max-time`）。接続後に応答を返さない host で nightly record job が
/// 子プロセス完了待ちのまま job timeout（60分）まで止まるのを防ぐ（finding 3368730838）。
const MAX_TIME_SECS: &str = "30";
/// 安全 fetch の最大ダウンロードサイズ（`--max-filesize`、バイト）。許可 host 上の巨大 raw ファイルを
/// 全量メモリへ読み込む前に転送自体を打ち切り、nightly record job のメモリ/時間消費を抑える（finding 3369076728）。
/// LLM へ渡す上限と同程度（2 MiB）に設定する（呼び出し側 adapter の truncate 上限より大きく取りすぎない）。
const MAX_FILESIZE_BYTES: &str = "2097152";

/// 安全 fetch（redirect 不追従・https 限定・有界 timeout/サイズ）の curl 引数列を組み立てる純粋関数。
///
/// **redirect を追従しない**こと（`--location` を含めず `--max-redirs 0`）が host allowlist 契約の要であり、
/// 引数列をテストで固定して退行を防ぐ。`-L` 無しのため curl は初期 URL の host 以外を踏まず、これが allowlist
/// 契約（allowlist 外へ踏まない）を保証する。`--fail` は 4xx/5xx を失敗にする（3xx は `--fail` では失敗にならず、
/// `-L` 無しのため `Location` も追従しないが、3xx でも本文自体は返り得る（サーバ実装次第）。それは許可済み host
/// からの本文であり、非空なら `Some` を返すのは support の責務で、内容は後段の機械バリデートで扱う）。`--proto =https`
/// で https 以外の scheme を拒む。
///
/// **有界化（finding 3368730838 / 3369076728）**: 取得失敗を `None` へ縮退させる設計に合わせ、ネットワーク不調や
/// 巨大ファイルで record job が止まらないよう、`--connect-timeout`（接続上限）・`--max-time`（転送全体の上限）・
/// `--max-filesize`（最大ダウンロードサイズ）を渡す。timeout/サイズ超過は curl の非 0 終了になり、呼び出し側は
/// 他の取得失敗と同様に `None` へ縮退する（record は止めない）。
pub(in crate::update_history) fn safe_fetch_args(url: &str) -> [OsString; 14] {
    [
        OsString::from("--fail"),
        OsString::from("--silent"),
        OsString::from("--show-error"),
        // redirect を追従しない（allowlist 外 host への横滑りを塞ぐ）。`-L` 無しのため 3xx の `Location` は
        // 追従されず、本文を返すのは常に初期 URL（allowlist 検査済み host）に限られる。3xx でも本文自体は返り
        // 得る（サーバ実装次第）が許可済み host からの本文であり、後段の機械バリデートで扱う（`--fail` は
        // 4xx/5xx 対象で 3xx は失敗にしない）。`--max-redirs 0` は「リダイレクトを 0 回まで」とする補強。
        OsString::from("--max-redirs"),
        OsString::from("0"),
        OsString::from("--proto"),
        OsString::from("=https"),
        // 接続上限・転送全体の上限（応答を返さない host で record job を止めない）。
        OsString::from("--connect-timeout"),
        OsString::from(CONNECT_TIMEOUT_SECS),
        OsString::from("--max-time"),
        OsString::from(MAX_TIME_SECS),
        // 最大ダウンロードサイズ（巨大 raw ファイルを全量読み込む前に転送を打ち切る）。
        OsString::from("--max-filesize"),
        OsString::from(MAX_FILESIZE_BYTES),
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

#[cfg(test)]
mod tests {
    //! 安全 fetch 引数列の redirect 不追従・https 限定（host allowlist 契約をコードで保証）と有界 timeout/サイズを
    //! hermetic に固定する。host 集合判定（domain）と安全 fetch（この support）の合成は呼び出し側 adapter の責務
    //! であり、その合成テストは adapter（github_models）側が持つ（本 module は domain へ依存しない純技術境界）。

    use super::safe_fetch_args;

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
    fn safe_fetch_args_bound_time_and_size() {
        // 退行固定（finding 3368730838 / 3369076728）: 安全 fetch は応答しない host や巨大ファイルで record job が
        // 止まらないよう、接続上限（`--connect-timeout`）・転送全体上限（`--max-time`）・最大サイズ
        // （`--max-filesize`）を有界で渡す。値の存在と「直後に有界な値が続く」ことを引数列で固定する。
        let args: Vec<String> = safe_fetch_args("https://github.com/a/b")
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        // 各 flag の直後に空でない有界値が続くこと（数値の正確値は定数側の責務）。
        for flag in ["--connect-timeout", "--max-time", "--max-filesize"] {
            let idx = args.iter().position(|arg| arg == flag);
            assert!(idx.is_some(), "{flag} を指定する: {args:?}");
            let value = idx.and_then(|i| args.get(i + 1)).map(String::as_str);
            assert!(
                value.is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())),
                "{flag} の直後に有界な数値が続く: {args:?}"
            );
        }
    }
}

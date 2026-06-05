//! `NotesPort` をリリースノートの HTTPS 取得（curl プロセス）へ接続する adapter。
//!
//! 更新パッケージの生リリースノートを forge releases / cask homepage から取得する境界である。取得は
//! 許可ホスト（github.com 等）の https URL に限定し、`process::run_capture` 経由の `curl` で本文を読む
//! （`dotfiles` の async runtime 内から blocking HTTP client を使わず、外部 curl へ翻訳する）。取得した
//! 本文は信頼境界外（prompt injection 源）であり、構造化・要約はせず生テキストのまま返す。後段の機械
//! バリデート（host/長さ/件数）と LLM 抽出は別責務である。
//!
//! ノート URL の解決: package 名から forge/cask URL を引く確定的手段（nixpkgs `meta.changelog` 評価等）は
//! 実行環境に依存するため、本 adapter は CI が解決した URL テンプレート（`notes_base`）に package 名を
//! 連結した URL を取得対象にする。`notes_base` 未指定時は URL を決められないため全 package で `None`（ノート
//! 無し）へ縮退する（version+notes_url へ縮退するプラン契約に沿う graceful degradation）。取得は許可ホスト
//! https に限定し、取得失敗（不通・404）も `None` へ縮退して record を止めない。

use std::ffi::OsString;

use crate::Result;
use crate::process::run_capture;
use crate::update_history::domain::validate::is_allowed_url;
use crate::update_history::ports::{NotesPort, RawReleaseNotes};

/// リリースノート取得を `NotesPort` 契約へ翻訳する adapter。
///
/// `notes_base` は CI が解決したノート URL の基底（末尾に package 名を連結して取得対象 URL を作る）。
/// `None` のとき URL を決められないとみなし、全 package で `None`（ノート無し）へ縮退する。
#[derive(Default)]
pub(in crate::update_history) struct ReleaseNotesAdapter {
    /// ノート URL の基底。`<notes_base><name>` を取得対象にする。未設定ならノート取得は縮退。
    notes_base: Option<String>,
}

impl ReleaseNotesAdapter {
    /// ノート URL の基底を束ねた adapter を作る。`None` でノート取得を縮退（`None`）にする。
    pub(in crate::update_history) fn new(notes_base: Option<String>) -> Self {
        Self { notes_base }
    }

    /// 許可ホスト https URL から本文を curl で取得する。
    ///
    /// 取得失敗（ネットワーク不通・404 等）は record を止めないよう `None` へ縮退する。URL が許可ホスト
    /// https でない場合は取得を試みず `None` を返す（信頼境界外 URL を踏まない）。`--proto =https` で
    /// https 以外の redirect 追従を禁止する。
    fn fetch(url: &str) -> Result<Option<RawReleaseNotes>> {
        if !is_allowed_url(url) {
            return Ok(None);
        }
        let args = [
            OsString::from("--fail"),
            OsString::from("--silent"),
            OsString::from("--show-error"),
            OsString::from("--location"),
            OsString::from("--proto"),
            OsString::from("=https"),
            OsString::from(url),
        ];
        match run_capture("curl", args) {
            Ok(text) if !text.trim().is_empty() => Ok(Some(RawReleaseNotes {
                text,
                notes_url: url.to_string(),
            })),
            // 空本文または取得失敗はノート無しとして縮退する。
            Ok(_) | Err(_) => Ok(None),
        }
    }
}

impl NotesPort for ReleaseNotesAdapter {
    fn fetch_release_notes(
        &self,
        name: &str,
        _old: Option<String>,
        _new: Option<String>,
    ) -> Result<Option<RawReleaseNotes>> {
        // ノート URL の基底が無い実行環境では URL を決められないため `None` へ縮退する。
        let Some(base) = &self.notes_base else {
            return Ok(None);
        };
        let url = format!("{base}{name}");
        Self::fetch(&url)
    }
}

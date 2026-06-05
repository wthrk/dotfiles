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
//!
//! **Homebrew cask の URL 解決（letter subdir）**: cask tap のファイルは `Casks/<name>.rb` ではなく
//! `Casks/<letter>/<name>.rb`（letter = name 先頭 1 文字の小文字）に配置される。`notes_base` を `Casks/` で
//! 終わる cask tap 基底にしたまま `<base><name>` を連結すると `Casks/<name>` を取得して**常に 404**になり、
//! ノートが空縮退する。よって基底が cask の `Casks/` レイアウトを指すときは `<base><letter>/<name>.rb` を
//! 構築する（[`resolve_notes_url`]）。それ以外の基底（forge 等）は従来どおり `<base><name>` を使う。

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
        let url = resolve_notes_url(base, name);
        Self::fetch(&url)
    }
}

/// `notes_base` と package 名から取得対象 URL を構築する純粋関数。
///
/// 基底が Homebrew cask tap の `Casks/` レイアウト（パスが `/Casks/` を含み `Casks/` で終わる）を指すときは、
/// cask の実配置 `Casks/<letter>/<name>.rb`（letter = name 先頭 1 文字を小文字化）を構築する。これにより
/// `<base><name>`（= `Casks/<name>`）を取得して常に 404 になり空縮退する不具合を避け、cask 経路で実取得が
/// 成立する。先頭文字が ASCII 英字でない（数字等の）cask は GitHub の cask tap が同様に小文字 1 文字 subdir に
/// 置くため、そのまま小文字化した 1 文字を letter に使う。cask 以外の基底（forge 等）は従来どおり
/// `<base><name>` を連結する。
fn resolve_notes_url(base: &str, name: &str) -> String {
    if is_cask_base(base) {
        let letter = cask_letter(name);
        format!("{base}{letter}/{name}.rb")
    } else {
        format!("{base}{name}")
    }
}

/// 基底が Homebrew cask tap の `Casks/` ディレクトリを指すか（`Casks/` で終わる）を判定する。
fn is_cask_base(base: &str) -> bool {
    base.ends_with("/Casks/") || base == "Casks/"
}

/// cask 名から letter subdir（先頭 1 文字を小文字化）を返す。空名なら空（呼び出し側で 404 縮退に倒れる）。
fn cask_letter(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_ascii_lowercase().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    //! cask の `Casks/<letter>/<name>.rb` URL 構築（P2-3 退行固定）と、非 cask 基底での従来連結を固定する。

    use super::resolve_notes_url;

    #[test]
    fn cask_base_resolves_letter_subdir_and_rb_suffix() {
        // P2-3 退行固定: `Casks/` で終わる cask 基底は `<base><letter>/<name>.rb` を構築する。
        // 旧実装の `<base><name>`（= `Casks/firefox`）は常に 404 でノートが空縮退していた。
        let base = "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/";
        assert_eq!(
            resolve_notes_url(base, "firefox"),
            "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/f/firefox.rb"
        );
        // 先頭が大文字でも letter は小文字化する（cask tap は小文字 1 文字 subdir）。
        assert_eq!(
            resolve_notes_url(base, "Discord"),
            "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/d/Discord.rb"
        );
        // 先頭が数字の cask はその文字をそのまま subdir に使う。
        assert_eq!(
            resolve_notes_url(base, "1password"),
            "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/1/1password.rb"
        );
    }

    #[test]
    fn non_cask_base_uses_plain_concatenation() {
        // cask レイアウトでない基底（forge 等）は従来どおり `<base><name>` を連結する。
        let base = "https://github.com/neovim/neovim/releases/tag/v";
        assert_eq!(
            resolve_notes_url(base, "0.11.0"),
            "https://github.com/neovim/neovim/releases/tag/v0.11.0"
        );
    }
}

//! `NotesPort` をリリースノートの HTTPS 取得（curl プロセス）へ接続する adapter。
//!
//! 更新パッケージの生リリースノートを forge releases / cask homepage から取得する境界である。取得は
//! 許可ホスト（github.com 等）の https URL に限定し、`process::run_capture` 経由の `curl` で本文を読む
//! （`dotfiles` の async runtime 内から blocking HTTP client を使わず、外部 curl へ翻訳する）。取得した
//! 本文は信頼境界外（prompt injection 源）であり、構造化・要約はせず生テキストのまま返す。後段の機械
//! バリデート（host/長さ/件数）と LLM 抽出は別責務である。
//!
//! ノート URL の解決は差分の出所（nix / brew）で分かれる:
//! - **nix eval 由来**: 各パッケージの `meta.changelog`（無ければ `meta.homepage`）を CI が `nix eval` で
//!   解決し、その URL を delta の `notes_source` として運ぶ。本 adapter は `notes_source` をそのまま取得対象に
//!   する（package 名連結はしない。changelog/homepage は既に完全な URL のため）。`notes_source` 不明
//!   （`meta` 不在）なら `None`（ノート無し）へ縮退する。
//! - **brew tap 由来**: CI が解決した cask tap の `Casks/` レイアウト base に package 名を連結した URL
//!   （`<base><letter>/<name>.rb`）を取得対象にする。`brew_notes_base` 未指定なら `None` へ縮退する。
//!
//! 出所で解決規則を分けるのは、nix と brew で取得先 URL の解決規則が異なり、混同すると誤った URL（例:
//! nix package を cask レイアウトで引いて 404）になるためである。いずれの取得先未解決もその package で
//! `None`（ノート無し）へ縮退する（version+notes_url へ縮退するプラン契約に沿う graceful degradation）。
//! 取得は許可ホスト https に限定し（`notes_source` は信頼境界外 URL のため `is_allowed_url` で機械検証）、
//! 取得失敗（不通・404）も `None` へ縮退して record を止めない。
//!
//! **Homebrew cask の URL 解決（letter / font subdir）**: cask tap のファイルは `Casks/<name>.rb` ではなく
//! `Casks/<letter>/<name>.rb`（letter = name 先頭 1 文字の小文字）に配置される。base を `Casks/` で
//! 終わる cask tap 基底にしたまま `<base><name>` を連結すると `Casks/<name>` を取得して**常に 404**になり、
//! ノートが空縮退する。よって基底が cask の `Casks/` レイアウトを指すときは `<base><letter>/<name>.rb` を
//! 構築する（[`resolve_notes_url`]）。例外として **font cask（名が `font-` で始まる）は `Casks/font/<name>.rb`**
//! （letter でなく `font` 固定サブディレクトリ）に置かれるため、font cask は subdir を `font` にする。それ以外の
//! 基底（forge 等）は従来どおり `<base><name>` を使う。

use std::ffi::OsString;

use crate::Result;
use crate::process::run_capture;
use crate::update_history::domain::diff::DeltaSource;
use crate::update_history::domain::validate::is_allowed_url;
use crate::update_history::ports::{NotesPort, RawReleaseNotes};

/// リリースノート取得を `NotesPort` 契約へ翻訳する adapter。
///
/// nix eval 由来 package のノート取得先は delta が運ぶ `notes_source`（`meta.changelog`/`meta.homepage`）を
/// 使うため adapter は base を持たない。`brew_notes_base` は CI が解決した brew cask の `Casks/` レイアウト
/// 基底（末尾に `<letter>/<name>.rb` を連結して取得対象 URL を作る）であり、`None` のとき brew package は
/// `None`（ノート無し）へ縮退する。
#[derive(Default)]
pub(in crate::update_history) struct ReleaseNotesAdapter {
    /// brew tap 由来 cask のノート URL 基底（cask 定義の `Casks/` レイアウト）。
    brew_notes_base: Option<String>,
}

impl ReleaseNotesAdapter {
    /// brew cask のノート URL 基底を束ねた adapter を作る。`None` なら brew package のノート取得を縮退する。
    /// nix eval 由来 package は delta の `notes_source` を取得先にするため base 引数を取らない。
    pub(in crate::update_history) fn new(brew_notes_base: Option<String>) -> Self {
        Self { brew_notes_base }
    }

    /// 許可ホスト https URL から本文を curl で取得する。
    ///
    /// 取得失敗（ネットワーク不通・404 等）は record を止めないよう `None` へ縮退する。URL が許可ホスト
    /// https でない場合は取得を試みず `None` を返す（信頼境界外 URL を踏まない）。
    ///
    /// **redirect は追従しない**（`--location` を付けず、`--max-redirs 0` で明示禁止）。`is_allowed_url` は
    /// 初期 URL の host しか検査できず、`--location` で redirect を追従すると 3xx 応答経由で allowlist 外
    /// ホストから本文を取得しうる（`--proto =https` は scheme 制限であって host 制限ではない）。これは
    /// 「許可ホストの https のみを踏む」契約に反する。host allowlist 契約は **`-L`（`--location`）を付けない**
    /// ことで保たれる: redirect を追従しないため、curl は初期 URL の host（`is_allowed_url` で検査済み）以外へ
    /// 一切アクセスしない。`--max-redirs 0` はその意図を明示的に固定する補強である。3xx 応答そのものは curl の
    /// `--fail`（通常 4xx/5xx を失敗にする）では失敗にならないが、`-L` 無しのため body を持たない 3xx 応答は
    /// 空本文として返り、`fetch` 側の「空本文 → `None`」分岐でノート空縮退（graceful degradation）になる。
    /// よって 3xx は allowlist 外 host を踏まず、かつ空縮退する。
    fn fetch(url: &str) -> Result<Option<RawReleaseNotes>> {
        if !is_allowed_url(url) {
            return Ok(None);
        }
        match run_capture("curl", curl_args(url)) {
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
        source: DeltaSource,
        notes_source: Option<String>,
        _old: Option<String>,
        _new: Option<String>,
    ) -> Result<Option<RawReleaseNotes>> {
        // 差分の出所に応じて取得対象 URL を解決する。nix eval 由来は delta が運ぶ notes_source（既に完全な
        // changelog/homepage URL）をそのまま使い、brew tap 由来は cask base + `<letter>/<name>.rb` を構築する。
        // 出所を取り違えると誤った URL（例: nix package を cask レイアウトで引いて 404）になるため振り分ける。
        // 取得対象 URL が解決できなければその package は `None`（ノート無し）へ縮退する。
        let url = match source {
            // nix eval 由来: meta.changelog/homepage 由来の notes_source をそのまま取得対象にする。
            // 不明（None）または空ならノート無しへ縮退する。
            DeltaSource::NixEval => {
                match notes_source
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(url) => url.to_string(),
                    None => return Ok(None),
                }
            }
            // brew tap 由来: cask base + name から `<base><letter>/<name>.rb` を構築する。base 未指定なら縮退。
            DeltaSource::BrewTap => {
                let Some(base) = &self.brew_notes_base else {
                    return Ok(None);
                };
                resolve_notes_url(base, name)
            }
        };
        Self::fetch(&url)
    }
}

/// curl 引数列を組み立てる純粋関数（redirect 不追従を引数列として固定検証できるよう実行から切り離す）。
///
/// **redirect を追従しない**こと（`--location` を含めず `--max-redirs 0`）が host allowlist 契約の要であり、
/// 引数列をテストで固定して退行を防ぐ。`-L` 無しのため curl は初期 URL の host 以外を踏まず、これが allowlist
/// 契約（allowlist 外へ踏まない）を保証する。`--fail` は 4xx/5xx を失敗にする（3xx は `--fail` では失敗にならず、
/// `-L` 無しのため追従もされず body 無しの 3xx として空縮退する）。`--proto =https` で https 以外の scheme を拒む。
fn curl_args(url: &str) -> [OsString; 8] {
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

/// `notes_base` と package 名から取得対象 URL を構築する純粋関数。
///
/// 基底が Homebrew cask tap の `Casks/` レイアウト（パスが `/Casks/` を含み `Casks/` で終わる）を指すときは、
/// cask の実配置 `Casks/<subdir>/<name>.rb` を構築する。subdir は通常 `<letter>`（name 先頭 1 文字を小文字化）
/// だが、**font cask（名が `font-` で始まる）は letter サブディレクトリでなく固定の `font` サブディレクトリ**
/// （`Casks/font/<name>.rb`）に置かれるため、font cask は subdir を `font` にする。これにより
/// `<base><name>`（= `Casks/<name>`）や `<base><letter>/<name>.rb`（font cask では 404）を取得して空縮退する
/// 不具合を避け、cask 経路で実取得が成立する。先頭文字が ASCII 英字でない（数字等の）cask は GitHub の cask
/// tap が同様に小文字 1 文字 subdir に置くため、そのまま小文字化した 1 文字を letter に使う。cask 以外の基底
/// （forge 等）は従来どおり `<base><name>` を連結する。
fn resolve_notes_url(base: &str, name: &str) -> String {
    if is_cask_base(base) {
        let subdir = cask_subdir(name);
        format!("{base}{subdir}/{name}.rb")
    } else {
        format!("{base}{name}")
    }
}

/// 基底が Homebrew cask tap の `Casks/` ディレクトリを指すか（`Casks/` で終わる）を判定する。
fn is_cask_base(base: &str) -> bool {
    base.ends_with("/Casks/") || base == "Casks/"
}

/// cask 名から配置 subdir を返す。font cask（`font-` 始まり）は固定 `font`、それ以外は先頭 1 文字の小文字。
///
/// Homebrew の font cask は `Casks/font/<name>.rb`（letter サブディレクトリでなく `font` 固定サブディレクトリ）
/// に置かれるため、`font-` 始まりの cask は subdir を `font` にする。それ以外は先頭 1 文字を小文字化した letter
/// subdir（`Casks/<letter>/<name>.rb`）。空名なら空（呼び出し側で 404 縮退に倒れる）。
fn cask_subdir(name: &str) -> String {
    if name.starts_with("font-") {
        return "font".to_string();
    }
    name.chars()
        .next()
        .map(|c| c.to_ascii_lowercase().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    //! cask の `Casks/<letter>/<name>.rb` URL 構築（P2-3 退行固定）と、非 cask 基底での従来連結、
    //! および curl の redirect 不追従引数列（S1 退行固定: host allowlist 契約をコードで保証）を固定する。

    use super::{ReleaseNotesAdapter, curl_args, resolve_notes_url};
    use crate::update_history::domain::diff::DeltaSource;
    use crate::update_history::ports::NotesPort;

    #[test]
    fn nix_without_notes_source_degrades_to_none_without_network() -> crate::Result<()> {
        // nix eval 由来 package の notes_source が不明（None）または空ならノート無しへ縮退し、curl を
        // 一切起動しない（hermetic: network 非依存）。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(
            adapter
                .fetch_release_notes("neovim", DeltaSource::NixEval, None, None, None)?
                .is_none()
        );
        assert!(
            adapter
                .fetch_release_notes(
                    "neovim",
                    DeltaSource::NixEval,
                    Some("   ".to_string()),
                    None,
                    None,
                )?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn nix_with_disallowed_notes_source_host_degrades_to_none() -> crate::Result<()> {
        // notes_source は信頼境界外 URL。許可ホスト外（meta.homepage が任意ドメインを指す等）なら
        // `fetch` の `is_allowed_url` で弾かれ、curl を踏まず `None` へ縮退する（host allowlist 契約）。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(
            adapter
                .fetch_release_notes(
                    "evilpkg",
                    DeltaSource::NixEval,
                    Some("https://evil.example/changelog".to_string()),
                    None,
                    None,
                )?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn brew_without_base_degrades_to_none() -> crate::Result<()> {
        // brew tap 由来で cask base 未指定ならノート無しへ縮退し、curl を起動しない。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(
            adapter
                .fetch_release_notes("firefox", DeltaSource::BrewTap, None, None, None)?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn curl_args_do_not_follow_redirects() {
        // S1 退行固定: redirect を追従すると `is_allowed_url` で検査した初期 host を越えて allowlist 外 host
        // から本文を取得しうる（host allowlist 契約違反）。`--location` を含めず `--max-redirs 0` を渡すこと、
        // および https に限定する `--proto =https` を引数列で固定する。
        let args: Vec<String> = curl_args("https://github.com/a/b")
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            !args.iter().any(|arg| arg == "--location" || arg == "-L"),
            "redirect を追従してはならない: {args:?}"
        );
        // `--max-redirs 0` が「0」値とともに含まれる。
        let max_redirs_idx = args
            .iter()
            .position(|arg| arg == "--max-redirs")
            .expect("--max-redirs を指定する");
        assert_eq!(args.get(max_redirs_idx + 1).map(String::as_str), Some("0"));
        // https 以外の scheme を拒む。
        let proto_idx = args
            .iter()
            .position(|arg| arg == "--proto")
            .expect("--proto を指定する");
        assert_eq!(args.get(proto_idx + 1).map(String::as_str), Some("=https"));
        // 取得対象 URL が末尾に乗る。
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://github.com/a/b")
        );
    }

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
    fn font_cask_resolves_font_subdir() {
        // N7 退行固定: font cask（`font-` 始まり）は letter subdir でなく固定 `font` subdir に置かれるため、
        // `Casks/font/<name>.rb` を構築する。letter subdir（`Casks/f/font-cica.rb`）は 404 になり空縮退していた。
        let base = "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/";
        assert_eq!(
            resolve_notes_url(base, "font-cica"),
            "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/font/font-cica.rb"
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

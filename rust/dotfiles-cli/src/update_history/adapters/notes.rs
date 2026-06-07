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
//!   解決し、その URL を delta の `notes_source` として運ぶ。ただし `meta.changelog`/`meta.homepage` は
//!   **github.com の HTML ページ URL**（`.../blob/<ref>/<path>` の changelog ファイル閲覧ページ、
//!   `.../releases/tag/<tag>` のリリースページ、repo root）であることが多く、そのまま curl すると生の
//!   リリースノートでなく **GitHub の HTML ページ**が返り、LLM が抽出材料にできず空配列になる。よって本
//!   adapter は `notes_source` を「生ノートテキストが返る取得先」へ [`resolve_nix_notes_source`] で変換して
//!   から取得する（package 名連結はしない）。変換規則は次のとおり:
//!   - `github.com/<owner>/<repo>/blob/<ref>/<path>` → `raw.githubusercontent.com/<owner>/<repo>/<ref>/<path>`
//!     （生ファイル取得。`/blob/refs/tags/<tag>/...` の ref 形も含めて `blob/` 直後をそのまま raw の ref 位置へ移す）。
//!   - `github.com/<owner>/<repo>/releases/tag/<tag>` → Releases API
//!     `api.github.com/repos/<owner>/<repo>/releases/tags/<tag>` を取得し JSON の `.body`（リリースノート
//!     markdown）を抽出して生ノートにする。
//!   - それ以外（repo root `github.com/<owner>/<repo>`、gitlab、判別不能）→ 生ノート取得不能として `None`
//!     （version+notes_url 縮退）。`notes_source` 不明（`meta` 不在）も `None` へ縮退する。
//!
//!   URL 形の翻訳（HTML 閲覧 URL → 生テキスト取得先）は外部取得先の形式差異吸収であり adapter の責務に置く。
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

    /// 取得方式（[`NotesFetchPlan`]）に従って生ノートを取得する。
    ///
    /// `Raw` は取得先 URL の本文をそのまま生ノートにする（[`Self::fetch`] と同じ host allowlist・redirect
    /// 不追従の取得経路）。`ReleasesApi` は Releases API JSON を取得し、`.body`（リリースノート markdown）を
    /// 生ノートとして取り出す。取得方式に関わらず取得先 URL は変換後 URL であり、`fetch`/`fetch_release_api`
    /// 内で `is_allowed_url`（raw.githubusercontent.com / api.github.com を含む allowlist）を機械適用する。
    /// 取得失敗・空本文・JSON 不正・`.body` 空はいずれもノート無し（version+notes_url 縮退）へ倒す。
    fn fetch_plan(plan: NotesFetchPlan) -> Result<Option<RawReleaseNotes>> {
        match plan {
            NotesFetchPlan::Raw(url) => Self::fetch(&url),
            NotesFetchPlan::ReleasesApi { api_url, notes_url } => {
                Self::fetch_release_api(&api_url, &notes_url)
            }
        }
    }

    /// GitHub Releases API JSON を取得し `.body`（リリースノート markdown）を生ノートとして返す。
    ///
    /// `api_url` は `is_allowed_url`（api.github.com を含む allowlist）で検査してから [`fetch`](Self::fetch)
    /// と同じ redirect 不追従経路で取得する。応答 JSON の `.body` フィールド（文字列）を抽出し、それを
    /// 生ノートテキストにする。記録に残す `notes_url` は表示用に元の `releases/tag` ページ URL を使う
    /// （API URL でなく人間が辿れるリリースページを残す）。取得失敗・JSON 不正・`.body` 不在/非文字列/空は
    /// すべてノート無しへ縮退する。`.body` は GitHub 上の任意入力であり信頼境界外のまま後段の機械バリデートで守る。
    ///
    /// token は不要（公開リポジトリの Releases API は未認証で読める）。本 adapter は token を付けないため
    /// argv/ログに secret は現れない。
    fn fetch_release_api(api_url: &str, notes_url: &str) -> Result<Option<RawReleaseNotes>> {
        if !is_allowed_url(api_url) {
            return Ok(None);
        }
        let json = match run_capture("curl", release_api_curl_args(api_url)) {
            Ok(text) if !text.trim().is_empty() => text,
            Ok(_) | Err(_) => return Ok(None),
        };
        match extract_release_body(&json) {
            Some(body) => Ok(Some(RawReleaseNotes {
                text: body,
                notes_url: notes_url.to_string(),
            })),
            None => Ok(None),
        }
    }
}

/// nix eval 由来 `notes_source`（信頼境界外 URL）の取得方式を表す純粋な解決結果。
///
/// `meta.changelog`/`meta.homepage` の HTML 閲覧 URL を「生ノートが返る取得先」へ翻訳した結果であり、
/// adapter はこれを受けて取得経路（生本文 / Releases API `.body`）を選ぶ。変換不能は呼び出し側で `None`
/// （version+notes_url 縮退）へ倒す。
enum NotesFetchPlan {
    /// 取得先 URL の本文をそのまま生ノートにする（raw ファイル等）。
    Raw(String),
    /// Releases API JSON を取得し `.body` を生ノートにする。`notes_url` は記録に残す元のリリースページ URL。
    ReleasesApi { api_url: String, notes_url: String },
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
            // nix eval 由来: meta.changelog/homepage 由来の notes_source は github.com の HTML 閲覧ページ
            // URL のことが多く、そのまま curl すると HTML が返り LLM が抽出できない。生ノートが返る取得先へ
            // 変換する。不明（None）・空・変換不能（repo root/gitlab 等）はノート無しへ縮退する。
            DeltaSource::NixEval => {
                let Some(raw) = notes_source
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    return Ok(None);
                };
                match resolve_nix_notes_source(raw) {
                    Some(plan) => return Self::fetch_plan(plan),
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

/// Releases API 取得用の curl 引数列を組み立てる純粋関数。
///
/// 取得経路の host allowlist 契約（redirect 不追従・https 限定）は [`curl_args`] と同一で、加えて GitHub
/// REST API が要求/推奨する `Accept: application/vnd.github+json` ヘッダを付ける（JSON 応答を固定する）。
/// このヘッダは secret を含まないため argv に置いてよい（token は付けないため argv/ログに secret は現れない）。
fn release_api_curl_args(url: &str) -> [OsString; 10] {
    [
        OsString::from("--fail"),
        OsString::from("--silent"),
        OsString::from("--show-error"),
        OsString::from("--max-redirs"),
        OsString::from("0"),
        OsString::from("--proto"),
        OsString::from("=https"),
        // GitHub REST API の JSON 応答を固定する（secret 非含有のため argv 可）。
        OsString::from("--header"),
        OsString::from("Accept: application/vnd.github+json"),
        OsString::from(url),
    ]
}

/// nix eval 由来 `notes_source` URL を「生ノートが返る取得先」へ翻訳する純粋関数。
///
/// `meta.changelog`/`meta.homepage` は github.com の HTML 閲覧ページ URL であることが多く、そのまま curl
/// すると HTML が返り LLM が抽出できない。本関数は host=`github.com` の URL 形を判別して取得方式へ変換する:
/// - `/<owner>/<repo>/blob/<ref...>/<path...>` → `raw.githubusercontent.com/<owner>/<repo>/<ref...>/<path...>`
///   （`/blob/` 直後の ref+path 部分をそのまま raw のパスへ移す。`blob/refs/tags/<tag>/CHANGELOG.md` のような
///   ref 形も `blob/` の後ろを保つことで正しく変換される）。[`NotesFetchPlan::Raw`] で生ファイル取得。
/// - `/<owner>/<repo>/releases/tag/<tag>` → `api.github.com/repos/<owner>/<repo>/releases/tags/<tag>` を
///   [`NotesFetchPlan::ReleasesApi`] で取得し `.body` を生ノートにする（記録用 `notes_url` は元のリリースページ）。
/// - repo root（`/<owner>/<repo>` のみ）・github.com 以外（gitlab 等）・判別不能 → `None`（取得不能縮退）。
///
/// host を含む URL の厳密判定のみを行い、取得そのものは行わない（純粋関数）。変換後 URL の host allowlist
/// 検査は取得側（`fetch`/`fetch_release_api`）が `is_allowed_url` で行う。
fn resolve_nix_notes_source(url: &str) -> Option<NotesFetchPlan> {
    // github.com の https URL のパス部分だけを対象にする。それ以外（gitlab 等）は取得不能縮退。
    let rest = url.strip_prefix("https://github.com/")?;
    // path injection / credential 混入を避けるため authority 末尾（`/` 区切り）以降だけを見る。
    // strip 済みなので rest は path（`<owner>/<repo>/...`）。owner/repo を切り出す。
    let mut segments = rest.splitn(3, '/');
    let owner = non_empty(segments.next())?;
    let repo = non_empty(segments.next())?;
    let tail = segments.next().unwrap_or("");

    if let Some(blob_tail) = tail.strip_prefix("blob/") {
        // blob/<ref...>/<path...> の ref+path をそのまま raw のパスへ移す。
        if blob_tail.is_empty() {
            return None;
        }
        return Some(NotesFetchPlan::Raw(format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/{blob_tail}"
        )));
    }
    if let Some(tag) = tail.strip_prefix("releases/tag/") {
        let tag = non_empty(Some(tag))?;
        return Some(NotesFetchPlan::ReleasesApi {
            api_url: format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}"),
            notes_url: url.to_string(),
        });
    }
    // repo root（tail 空）や未対応のパス形は生ノート取得不能として縮退する。
    None
}

/// 文字列 option を trim して空でなければ返す（URL セグメントの非空判定用ヘルパ）。
fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// GitHub Releases API 応答 JSON から `.body`（リリースノート markdown）を抽出する純粋関数。
///
/// `.body` フィールドが文字列でかつ trim 後非空のときだけ `Some` を返す。JSON 不正・`.body` 不在・非文字列・
/// 空文字列はすべて `None`（取得不能縮退）。`.body` は GitHub 上の任意入力であり信頼境界外のまま返し、
/// 構造化・要約はしない（後段の機械バリデートで守る）。
fn extract_release_body(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let body = value.get("body")?.as_str()?;
    if body.trim().is_empty() {
        None
    } else {
        Some(body.to_string())
    }
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

    use super::{
        NotesFetchPlan, ReleaseNotesAdapter, curl_args, extract_release_body,
        release_api_curl_args, resolve_nix_notes_source, resolve_notes_url,
    };
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
    fn blob_url_resolves_to_raw_githubusercontent() {
        // changelog の HTML 閲覧 URL（blob）は raw ファイル取得先へ変換する。
        match resolve_nix_notes_source("https://github.com/o/r/blob/v1.2.3/CHANGELOG.md") {
            Some(NotesFetchPlan::Raw(url)) => assert_eq!(
                url,
                "https://raw.githubusercontent.com/o/r/v1.2.3/CHANGELOG.md"
            ),
            other => panic!("expected Raw, got {other:?}", other = PlanDbg(&other)),
        }
        // ref が `refs/tags/<tag>` 形でも blob 直後をそのまま raw のパスへ移す。
        match resolve_nix_notes_source(
            "https://github.com/o/r/blob/refs/tags/v1.2.3/docs/CHANGELOG.md",
        ) {
            Some(NotesFetchPlan::Raw(url)) => assert_eq!(
                url,
                "https://raw.githubusercontent.com/o/r/refs/tags/v1.2.3/docs/CHANGELOG.md"
            ),
            other => panic!("expected Raw, got {other:?}", other = PlanDbg(&other)),
        }
    }

    #[test]
    fn releases_tag_url_resolves_to_releases_api() {
        // releases/tag ページは Releases API へ変換し、記録 URL は元のリリースページを保つ。
        match resolve_nix_notes_source("https://github.com/o/r/releases/tag/v2.0.0") {
            Some(NotesFetchPlan::ReleasesApi { api_url, notes_url }) => {
                assert_eq!(
                    api_url,
                    "https://api.github.com/repos/o/r/releases/tags/v2.0.0"
                );
                assert_eq!(notes_url, "https://github.com/o/r/releases/tag/v2.0.0");
            }
            other => panic!(
                "expected ReleasesApi, got {other:?}",
                other = PlanDbg(&other)
            ),
        }
    }

    #[test]
    fn repo_root_and_non_github_and_unknown_degrade_to_none() {
        // repo root（生ノート不能）・github.com 以外（gitlab）・判別不能パスはすべて取得不能縮退（None）。
        assert!(resolve_nix_notes_source("https://github.com/o/r").is_none());
        assert!(resolve_nix_notes_source("https://github.com/o/r/").is_none());
        assert!(resolve_nix_notes_source("https://gitlab.com/o/r/blob/v1/CHANGELOG.md").is_none());
        assert!(resolve_nix_notes_source("https://github.com/o/r/tree/main/docs").is_none());
        assert!(resolve_nix_notes_source("https://example.com/whatever").is_none());
        // owner/repo 欠落・blob/releases 直後が空も縮退。
        assert!(resolve_nix_notes_source("https://github.com/o").is_none());
        assert!(resolve_nix_notes_source("https://github.com/o/r/blob/").is_none());
        assert!(resolve_nix_notes_source("https://github.com/o/r/releases/tag/").is_none());
    }

    #[test]
    fn extract_release_body_reads_body_field() {
        // Releases API JSON の `.body` を生ノートとして抽出する（fixture: 実 network 非依存）。
        let json = "{\"tag_name\":\"v1.0.0\",\"body\":\"## Fixes\\n- crash on startup\"}";
        assert_eq!(
            extract_release_body(json).as_deref(),
            Some("## Fixes\n- crash on startup")
        );
    }

    #[test]
    fn extract_release_body_degrades_on_missing_empty_or_invalid() {
        // `.body` 不在・非文字列・空・JSON 不正はすべて None（version+notes_url 縮退）。
        assert!(extract_release_body(r#"{"tag_name":"v1.0.0"}"#).is_none());
        assert!(extract_release_body(r#"{"body":null}"#).is_none());
        assert!(extract_release_body(r#"{"body":123}"#).is_none());
        assert!(extract_release_body(r#"{"body":"   "}"#).is_none());
        assert!(extract_release_body("not json").is_none());
    }

    #[test]
    fn release_api_curl_args_set_accept_and_no_redirects() {
        // Releases API 取得も redirect 不追従・https 限定を維持し、JSON 応答固定の Accept を付ける。
        let args: Vec<String> =
            release_api_curl_args("https://api.github.com/repos/o/r/releases/tags/v1")
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
        assert!(!args.iter().any(|arg| arg == "--location" || arg == "-L"));
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
        let header_idx = args
            .iter()
            .position(|arg| arg == "--header")
            .expect("--header を指定する");
        assert_eq!(
            args.get(header_idx + 1).map(String::as_str),
            Some("Accept: application/vnd.github+json")
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://api.github.com/repos/o/r/releases/tags/v1")
        );
    }

    #[test]
    fn nix_with_repo_root_notes_source_degrades_without_network() -> crate::Result<()> {
        // nix 経路で notes_source が repo root（変換不能）なら curl を起動せず None へ縮退する（hermetic）。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(
            adapter
                .fetch_release_notes(
                    "neovim",
                    DeltaSource::NixEval,
                    Some("https://github.com/neovim/neovim".to_string()),
                    None,
                    None,
                )?
                .is_none()
        );
        Ok(())
    }

    /// テスト失敗メッセージ用に [`NotesFetchPlan`] を簡易表示するラッパ（本体は `Debug` を持たない）。
    struct PlanDbg<'a>(&'a Option<NotesFetchPlan>);
    impl std::fmt::Debug for PlanDbg<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.0 {
                None => write!(f, "None"),
                Some(NotesFetchPlan::Raw(u)) => write!(f, "Raw({u})"),
                Some(NotesFetchPlan::ReleasesApi { api_url, notes_url }) => {
                    write!(
                        f,
                        "ReleasesApi {{ api_url: {api_url}, notes_url: {notes_url} }}"
                    )
                }
            }
        }
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

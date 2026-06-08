//! `NotesPort` をリリースノートの HTTPS 取得（curl プロセス）へ接続する adapter。
//!
//! 更新パッケージの生リリースノートを forge releases / cask homepage から取得する境界である。取得は
//! 許可ホスト（github.com 等）の https URL に限定し、`process::run_capture` 経由の `curl` で本文を読む
//! （`dotfiles` の async runtime 内から blocking HTTP client を使わず、外部 curl へ翻訳する）。取得した
//! 本文は信頼境界外（prompt injection 源）であり、構造化・要約はせず生テキストのまま返す。後段の機械
//! バリデート（host/長さ/件数）と LLM 抽出は別責務である。
//!
//! ノート取得は差分の出所（nix / brew）で分かれる:
//! - **nix eval 由来**: CI が `nix eval` で各パッケージの GitHub `owner/repo`（`meta.homepage`→`src`→
//!   `meta.changelog` の優先で抽出）と changelog URL を解決し、delta の `repo`/`notes_source` として運ぶ。
//!   本 adapter は **GitHub Releases API で old→new の version 範囲のリリースノートを取得**する経路を一次に
//!   置く（[`fetch_releases_range`]）:
//!   - `repo`（`owner/repo`）があれば `GET /repos/{o}/{r}/releases`（ページング）を **GITHUB_TOKEN 認証**で
//!     取得し（未認証 60 req/h を避け 5000 req/h に。token は stdin config（`--config -`）で渡し argv/ログ
//!     非露出）、各リリースの tag/version が `(old, new]` に入るものの `.body`（リリースノート markdown）を
//!     **古い順に連結**して生ノートにする。tag 名は `v{ver}`/`{ver}`/`{name}-v{ver}` 等の揺れがあるため
//!     正規化して version 一致を見る（範囲判定・tag 正規化は domain の `version_in_range`/`release_version`
//!     へ委譲し、adapter は JSON へ適用するだけ）。記録用 `notes_url` は人間が辿れる
//!     `github.com/{o}/{r}/releases` ページにする。
//!   - **フォールバック連鎖**: Releases API で取れない（リリース未公開・tag 不一致・404・repo 不明・token
//!     未設定）→ `notes_source`（`meta.changelog`/`meta.homepage`）を「生ノートが返る取得先」へ
//!     [`resolve_nix_notes_source`] で変換して取得（changelog blob→raw / releases/tag→Releases API）→
//!     それも不能なら `None`（version+notes_url 縮退）。サイレント全滅でなく graceful degrade。
//!
//!   `resolve_nix_notes_source` の変換規則（changelog フォールバック経路）:
//!   - `github.com/<owner>/<repo>/blob/<ref>/<path>` → `raw.githubusercontent.com/<owner>/<repo>/<ref>/<path>`
//!     （生ファイル取得。`/blob/refs/tags/<tag>/...` の ref 形も含めて `blob/` 直後をそのまま raw の ref 位置へ移す）。
//!   - `github.com/<owner>/<repo>/releases/tag/<tag>` → Releases API
//!     `api.github.com/repos/<owner>/<repo>/releases/tags/<tag>` を取得し JSON の `.body` を抽出して生ノートにする。
//!   - それ以外（repo root `github.com/<owner>/<repo>`、gitlab、判別不能）→ 生ノート取得不能として `None`。
//!
//!   URL 形の翻訳（HTML 閲覧 URL → 生テキスト取得先）は外部取得先の形式差異吸収であり adapter の責務に置く。
//! - **brew tap 由来**: cask `.rb` 定義は homepage/url を含む**定義ファイル**であって実リリースノート本文では
//!   ないため、`fetch_release_notes` の seed（summarize_only 経路）にしない（finding 3374863454。常に `None`）。
//!   代わりに [`ReleaseNotesAdapter::resolve_brew_notes_hint`] が CI 解決の cask tap `Casks/` レイアウト base に
//!   package 名を連結した URL（`<base><letter>/<name>.rb`）を取得し、Ruby の `homepage`/`url` を **探索ヒント**
//!   として取り出す。application はそのヒントを `ExtractRequest.homepage` へ載せ、AI tool-use 探索（agent_loop）に
//!   実ノートを探させる。`brew_notes_base` 未指定なら探索ヒントも `None` へ縮退する。
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
use crate::process::{run_capture, run_capture_with_stdin};
use crate::update_history::domain::diff::{
    DeltaSource, release_version, version_in_range, version_ordering,
};
use crate::update_history::domain::validate::is_allowed_url;
use crate::update_history::ports::{NotesPort, RawReleaseNotes};
use crate::update_history::support::safe_https_fetch;

/// curl の `--write-out` トレーラを本文から切り出す sentinel（Releases API 取得の HTTP status 読み取り用）。
///
/// 本文（任意の JSON 応答）と衝突しない一意文字列を選ぶ。`fetch_releases_page` は `--fail` を使わず
/// （4xx/5xx を curl exit へ倒すと本物の status を握り潰し 401/403/429 を空振りと区別できなくなる）、
/// 本文末尾へこの sentinel に続けて `http_code` を付加し [`split_status`] で本文・status へ切り分ける。
const RELEASES_META_SENTINEL: &str = "\n<<<DOTFILES_RELEASES_META>>>\n";

/// Releases API 取得 curl の接続上限秒（`--connect-timeout`）。応答しない host で待ち続けないため
/// （finding 3368730838）。support の安全 fetch と同程度に取る。
const FETCH_CONNECT_TIMEOUT_SECS: &str = "10";
/// Releases API 取得 curl の転送全体の上限秒（`--max-time`）。接続後に応答を返さない host で record job が
/// 子プロセス完了待ちのまま job timeout（60分）まで止まるのを防ぐ（finding 3368730838）。
const FETCH_MAX_TIME_SECS: &str = "30";

/// GitHub Releases API のページサイズ（1 ページあたりの最大件数。API 上限は 100）。
const RELEASES_PER_PAGE: u32 = 100;

/// Releases API のページング取得上限ページ数。`(old, new]` 範囲のリリースは通常 1 ページ（直近 100 件）に
/// 収まる（nightly bump の old→new は近接バージョン間）。無人パイプラインで 1 リポジトリに過大なリクエストを
/// 投げないよう有界にする（最大 RELEASES_PER_PAGE × MAX_RELEASE_PAGES 件まで走査）。
const MAX_RELEASE_PAGES: u32 = 3;

/// 範囲取得した複数リリース `.body` を連結する際の区切り。LLM へは古い順に積んだ生テキストとして渡す。
const RELEASE_BODY_SEPARATOR: &str = "\n\n---\n\n";

/// リリースノート取得を `NotesPort` 契約へ翻訳する adapter。
///
/// nix eval 由来 package のノート取得先は delta が運ぶ `notes_source`（`meta.changelog`/`meta.homepage`）を
/// 使うため adapter は base を持たない。`brew_notes_base` は CI が解決した brew cask の `Casks/` レイアウト
/// 基底（末尾に `<letter>/<name>.rb` を連結して cask 定義 URL を作る）であり、cask 定義から **探索ヒント**
/// （homepage/url）を取り出す [`resolve_brew_notes_hint`](Self::resolve_brew_notes_hint) が使う。cask 定義そのものは
/// seed にしない（finding 3374863454）。`None` のとき brew package は探索ヒント無し（version-only）へ縮退する。
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

    /// 許可ホスト https URL から本文を取得し、`RawReleaseNotes` へ翻訳する。
    ///
    /// host allowlist 検査（`is_allowed_url`、初期 URL の host のみ検査できる domain rule）をこの adapter が
    /// 行い、許可ホスト https でない URL は取得を試みず `None` を返す（信頼境界外 URL を踏まない）。実 curl は
    /// process-generic な安全 fetch primitive（[`safe_https_fetch`]、`-L` 無し・`--max-redirs 0`・`--proto =https`・
    /// 非空本文のみ `Some`）へ委譲する。redirect 不追従が host allowlist 契約の要であり、support 側が引数列で
    /// 固定する（`-L` で redirect を追従すると 3xx 経由で allowlist 外 host から本文を取得しうるが、`-L` 無しの
    /// ため初期 host 以外を踏まない。3xx でも本文自体は返り得る（サーバ実装次第）が、それは許可済み host からの
    /// 本文であり、非空本文を `Some` として返すかは support 側の責務、内容は後段の機械バリデートで扱う）。取得失敗・空本文は record を
    /// 止めないよう `None` へ縮退する。adapter は host 検査と `RawReleaseNotes`（記録 URL 付与）への翻訳だけを担い、
    /// 安全 curl の引数組み立ては support に閉じる（複数 adapter での curl 引数二重実装を避ける）。
    fn fetch(url: &str) -> Result<Option<RawReleaseNotes>> {
        if !is_allowed_url(url) {
            return Ok(None);
        }
        Ok(safe_https_fetch(url)?.map(|text| RawReleaseNotes {
            text,
            notes_url: url.to_string(),
            // `fetch` は `url` の raw 本文をそのまま返すため、同じ `url` を再取得すれば同じ本文が返る
            // （raw changelog ファイル・cask `.rb` 生ファイル）。よって `url` を再取得用 source として学習してよい
            // （finding 3369076722）。
            refetch_url: Some(url.to_string()),
        }))
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
                // 本文は Releases API JSON の `.body` から抽出したものであり、`notes_url`（表示用リリースページ）
                // も `api_url`（JSON 応答）も raw 取得では同じ本文を返さない。再取得用 source を持たない
                // （finding 3369076722）→ record は再利用 source を学習せず、次回も機械解決し直す。
                refetch_url: None,
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
        // brew は cask `.rb` を seed にしないため name を使わず（探索ヒントは resolve_brew_notes_hint が name で引く）、
        // nix は repo/notes_source で取得元を解決するため、本関数では name を使わない。
        _name: &str,
        source: DeltaSource,
        repo: Option<String>,
        notes_source: Option<String>,
        old: Option<String>,
        new: Option<String>,
    ) -> Result<Option<RawReleaseNotes>> {
        // 差分の出所に応じて取得元を振り分ける。出所を取り違えると誤った URL（例: nix package を cask
        // レイアウトで引いて 404）になるため分ける。どの取得元も解決・取得不能ならその package は `None`
        // （ノート無し）へ縮退する。
        match source {
            // nix eval 由来: 一次に GitHub Releases API で old→new 範囲のリリースノートを取得し、空振り時は
            // changelog（meta.changelog/homepage）へフォールバックする。両方不能ならノート無しへ縮退。
            DeltaSource::NixEval => Self::fetch_nix_notes(repo, notes_source, old, new),
            // brew tap 由来: cask `.rb` 定義そのものは実ノート本文ではなく homepage/url を含む定義ファイルなので、
            // seed（summarize_only 経路）にしない（finding 3374863454）。seed は常に `None` を返し、homepage 探索
            // ヒントは [`resolve_brew_notes_hint`](Self::resolve_brew_notes_hint) 経由で application が agent_loop へ
            // 回す。これにより cask 経路は定義ファイルを直接要約せず、AI に実ノートを探索させる。
            DeltaSource::BrewTap => Ok(None),
        }
    }

    /// レジストリに保存済みの取得元 URL を直接 fetch して生ノートを取得する（再利用フロー専用）。
    ///
    /// 既存の [`fetch`](Self::fetch) 経路をそのまま使い、`is_allowed_url`（host allowlist）+ redirect 不追従
    /// （`-L` 無し・`--max-redirs 0`）+ `--proto =https` を機械適用する。`url` はレジストリ由来（AI-discovered
    /// なら元は AI 由来）の信頼境界外 URL だが、この機械検証を必ず通すことで許可外 host を踏まない。取得失敗・
    /// 空本文・許可外 host はいずれも `None`（呼び出し側は自己修復として機械解決 → AI 探索へフォールバックする）。
    fn fetch_notes_from_source(&self, url: &str) -> Result<Option<RawReleaseNotes>> {
        Self::fetch(url)
    }

    /// brew cask `.rb` 定義を取得し、`homepage`（無ければ `url`）を探索ヒント URL として 1 件取り出す。
    ///
    /// cask 定義そのものは実ノート本文でないため seed にしない（finding 3374863454）。base + name から
    /// `Casks/<subdir>/<name>.rb` を構築（[`resolve_notes_url`]）して [`fetch`](Self::fetch) と同じ host
    /// allowlist + redirect 不追従経路で取得し、Ruby の `homepage "..."`/`url "..."` の値を抽出する
    /// （[`parse_cask_hint`]）。base 未指定・取得失敗・抽出不能はすべて `None`（探索ヒント無し）。抽出した URL は
    /// 信頼境界外であり、application が `ExtractRequest.homepage` へ載せた後の agent_loop 側 fetch 許可ホスト
    /// 検査・SSRF 防御で守る。
    fn resolve_brew_notes_hint(&self, name: &str) -> Result<Option<String>> {
        let Some(base) = &self.brew_notes_base else {
            return Ok(None);
        };
        let url = resolve_notes_url(base, name);
        if !is_allowed_url(&url) {
            return Ok(None);
        }
        Ok(safe_https_fetch(&url)?.as_deref().and_then(parse_cask_hint))
    }
}

impl ReleaseNotesAdapter {
    /// nix eval 由来 package の生ノートを取得する。Releases API 範囲取得を一次に、changelog をフォールバックに、
    /// いずれも不能なら `None`（version+notes_url 縮退）にするフォールバック連鎖。
    ///
    /// 1. `repo`（`owner/repo`）があれば [`fetch_releases_range`] で `(old, new]` 範囲のリリースノートを
    ///    GITHUB_TOKEN 認証で取得する（取れたら返す）。
    /// 2. 取れなければ `notes_source`（`meta.changelog`/`meta.homepage`）を [`resolve_nix_notes_source`] で
    ///    生ノート取得先へ変換して取得する（取れたら返す）。
    /// 3. それも不能なら `None`。
    fn fetch_nix_notes(
        repo: Option<String>,
        notes_source: Option<String>,
        old: Option<String>,
        new: Option<String>,
    ) -> Result<Option<RawReleaseNotes>> {
        // 1. Releases API 範囲取得（一次）。repo が owner/repo 形で取れているときだけ試す。
        if let Some(repo) = repo.as_deref().map(str::trim).filter(|s| !s.is_empty())
            && let Some((owner, repo_name)) = split_owner_repo(repo)
            && let Some(notes) =
                Self::fetch_releases_range(owner, repo_name, old.as_deref(), new.as_deref())?
        {
            return Ok(Some(notes));
        }
        // 2. changelog（meta.changelog/homepage）フォールバック。HTML 閲覧 URL を生ノート取得先へ変換する。
        if let Some(raw) = notes_source
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && let Some(plan) = resolve_nix_notes_source(raw)
        {
            return Self::fetch_plan(plan);
        }
        // 3. いずれも不能。version+notes_url へ縮退。
        Ok(None)
    }

    /// GitHub Releases API で `owner/repo` の `(old, new]` 範囲のリリースノートを取得して連結する。
    ///
    /// `GET /repos/{owner}/{repo}/releases?per_page=...&page=...` を [`MAX_RELEASE_PAGES`] まで GITHUB_TOKEN
    /// 認証で取得し（token は stdin config で渡し argv/ログ非露出。未設定なら未認証で取得を試みる）、各リリースの
    /// tag/version が `(old, new]` に入るものの `.body` を **古い順に連結**して生ノートにする。範囲に入る
    /// リリースが 1 件も無い・API が取れない（404・不通・空応答・JSON 不正）は `None`（呼び出し側が changelog
    /// フォールバックへ倒す）。記録用 `notes_url` は人間が辿れる `github.com/{owner}/{repo}/releases`。
    ///
    /// `api.github.com` は [`is_allowed_url`] の allowlist 済み host。redirect 不追従・`--proto =https` を
    /// 適用する（[`releases_list_curl_args`]）。`.body` は信頼境界外のまま返し、構造化・要約はしない。
    ///
    /// 観測: 認証/レート失敗（HTTP 401/403/429）は単なる空振り（404・リリース無・範囲 0 件）と区別して
    /// stderr へ 1 行診断する（[`fetch_releases_page`]）。token 失効や per-hour レート枯渇による全件失敗を
    /// CI ログで判別可能にし、サイレント全滅を防ぐためである（token は露出しない）。
    fn fetch_releases_range(
        owner: &str,
        repo: &str,
        old: Option<&str>,
        new: Option<&str>,
    ) -> Result<Option<RawReleaseNotes>> {
        let token = github_token();
        let mut bodies: Vec<(String, String)> = Vec::new();
        for page in 1..=MAX_RELEASE_PAGES {
            let api_url = releases_list_url(owner, repo, page);
            // 変換後 URL の host allowlist を機械適用してから取得する（信頼境界外 URL を踏まない）。
            if !is_allowed_url(&api_url) {
                return Ok(None);
            }
            let json = match fetch_releases_page(&api_url, token.as_deref(), owner, repo) {
                Ok(Some(text)) => text,
                // 取得失敗（404・不通・空応答・認証/レート失敗）は changelog フォールバックへ倒す。
                // 認証/レート失敗（401/403/429）は fetch_releases_page 内で診断ログ済み。
                Ok(None) | Err(_) => return Ok(None),
            };
            let releases = match parse_releases(&json) {
                Some(releases) => releases,
                None => return Ok(None),
            };
            let page_len = releases.len();
            for release in releases {
                if let Some(version) = release.in_range_version(old, new) {
                    bodies.push((version, release.body));
                }
            }
            // 1 ページが満杯でなければ最終ページ（以降は空）なので打ち切る（過剰リクエスト抑制）。
            if (page_len as u32) < RELEASES_PER_PAGE {
                break;
            }
        }
        let text = join_release_bodies(bodies);
        if text.is_empty() {
            return Ok(None);
        }
        Ok(Some(RawReleaseNotes {
            text,
            notes_url: releases_page_url(owner, repo),
            // 本文は `(old, new]` 範囲の複数リリース `.body` を連結したもので、単一 raw URL では再現できない
            // （`notes_url` は表示用 HTML ページ）。再取得用 source を持たない（finding 3369076722）→ record は
            // 再利用 source を学習せず、次回も Releases API range を解決し直す。
            refetch_url: None,
        }))
    }
}

/// 範囲内 release の `(version, body)` 列を version 昇順（古い順）に連結する純粋関数。
///
/// Releases API は新しい順で返すため、version をキーに安定整列してから [`RELEASE_BODY_SEPARATOR`]
/// （`\n\n---\n\n`）で連結する。整列順は domain の [`version_ordering`]（成分単位比較）へ委譲する。
/// version 順序は domain rule であり、adapter が `String` 字句比較で再決定すると二桁 version で順序が
/// 逆転する（例: 字句では `"10.0.0" < "2.0.0"`）。adapter に version 比較を再実装せず domain 呼び出しのみ
/// にすることで層境界を保つ。空入力は空文字を返す（呼び出し側が `None` 縮退に倒す）。LLM へは古い順に
/// 積んだ生テキストとして渡す（時系列で読めるようにする）ための整列・連結規則。
fn join_release_bodies(mut bodies: Vec<(String, String)>) -> String {
    bodies.sort_by(|a, b| version_ordering(&a.0, &b.0));
    bodies
        .into_iter()
        .map(|(_, body)| body)
        .collect::<Vec<_>>()
        .join(RELEASE_BODY_SEPARATOR)
}

/// Releases API 取得用の curl 引数列を組み立てる純粋関数。
///
/// 取得経路の host allowlist 契約（redirect 不追従・https 限定）は support の安全 fetch 引数列
/// （[`safe_fetch_args`](crate::update_history::support::safe_fetch_args)）と同一で、加えて GitHub
/// REST API が要求/推奨する `Accept: application/vnd.github+json` ヘッダを付ける（JSON 応答を固定する）。
/// このヘッダは secret を含まないため argv に置いてよい（token は付けないため argv/ログに secret は現れない）。
fn release_api_curl_args(url: &str) -> [OsString; 14] {
    [
        OsString::from("--fail"),
        OsString::from("--silent"),
        OsString::from("--show-error"),
        OsString::from("--max-redirs"),
        OsString::from("0"),
        OsString::from("--proto"),
        OsString::from("=https"),
        // 応答しない host で record job が止まらないよう接続/転送全体を有界化する（finding 3368730838）。
        OsString::from("--connect-timeout"),
        OsString::from(FETCH_CONNECT_TIMEOUT_SECS),
        OsString::from("--max-time"),
        OsString::from(FETCH_MAX_TIME_SECS),
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

/// `GITHUB_TOKEN` を読む。未設定/空なら `None`（未認証で Releases API 取得を試みる）。
///
/// token は Releases API の認証（5000 req/h）に使い、curl の stdin config 経由でのみ渡す（argv/ログ非露出）。
fn github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty())
}

/// `owner/repo` 文字列を `(owner, repo)` へ分割する純粋関数。
///
/// ちょうど 1 個の `/` で区切られ、両側が非空のときだけ `Some`。空・スラッシュ過不足・先頭末尾スラッシュは
/// `None`（owner/repo として不正 → Releases API を試みず changelog フォールバックへ倒す）。信頼境界外の値だが
/// URL 構築前に厳密な形判定を行い、path injection（余分な `/` で別エンドポイントを叩く）を塞ぐ。
fn split_owner_repo(repo: &str) -> Option<(&str, &str)> {
    let (owner, name) = repo.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some((owner, name))
}

/// Releases 一覧 API の URL を組み立てる純粋関数（`GET /repos/{owner}/{repo}/releases`）。
fn releases_list_url(owner: &str, repo: &str, page: u32) -> String {
    format!(
        "https://api.github.com/repos/{owner}/{repo}/releases?per_page={RELEASES_PER_PAGE}&page={page}"
    )
}

/// 記録・表示用の人間が辿れるリリース一覧ページ URL を組み立てる純粋関数。
fn releases_page_url(owner: &str, repo: &str) -> String {
    format!("https://github.com/{owner}/{repo}/releases")
}

/// Releases 一覧 API を 1 ページ取得する。`Ok(Some(json))` は本文取得成功（HTTP 200）、`Ok(None)` は空振り
/// （404・リリース無・空応答）または認証/レート失敗（401/403/429）で、いずれも changelog フォールバックへ倒す。
///
/// token があれば curl の `--config -`（stdin）の Authorization ヘッダで認証して 5000 req/h を使う
/// （token は argv/ログに出さない。[`auth_config`]）。token が無ければ未認証で取得する（60 req/h）。redirect
/// 不追従・`--proto =https` を適用する（[`releases_list_curl_args`]）。GitHub REST API の JSON 応答固定のため
/// `Accept` ヘッダも付ける。
///
/// 観測: `--fail` は使わず（4xx/5xx を curl exit へ倒すと本物の HTTP status を握り潰し、認証/レート失敗を
/// 空振りと区別できない＝サイレント全滅を招く）、`--write-out` で本文末尾へ status を付加して [`split_status`]
/// で読む。**401/403/429 は認証/レート失敗**として stderr へ 1 行診断する（token 失効・per-hour レート枯渇に
/// よる全件失敗を CI ログで判別可能にする）。それ以外の非 200（404 等）は単なる空振りとして無言で `None`。
/// curl プロセス自体の失敗（spawn/不通）は `Err`（呼び出し側が changelog フォールバックへ倒す）。token は
/// status 出力に現れない（`%{http_code}` は数値のみ）ため診断ログに secret は漏れない。
fn fetch_releases_page(
    api_url: &str,
    token: Option<&str>,
    owner: &str,
    repo: &str,
) -> Result<Option<String>> {
    let args = releases_list_curl_args(api_url, token.is_some());
    let raw = match token {
        Some(token) => run_capture_with_stdin("curl", args, auth_config(token).as_bytes())?,
        None => run_capture("curl", args)?,
    };
    let (status, body) = split_status(&raw);
    match status {
        200 if !body.trim().is_empty() => Ok(Some(body)),
        // 認証/レート失敗は空振り（404・範囲 0 件）と区別して診断する。token 失効・レート枯渇による全件失敗を
        // CI ログで判別可能にする（サイレント切り捨て禁止）。token は status に現れず secret は漏れない。
        401 | 403 | 429 => {
            eprintln!(
                "update-history notes: releases API auth/rate failure: HTTP {status} for {owner}/{repo}"
            );
            Ok(None)
        }
        // 200 だが空本文、または 404・その他非 200 は単なる空振り。無言で changelog フォールバックへ倒す。
        _ => Ok(None),
    }
}

/// curl の `--write-out` トレーラ（[`RELEASES_META_SENTINEL`] + status）出力から `(status, body)` を切り出す。
///
/// curl は本文を stdout へ流し、その後ろへ sentinel と `%{http_code}`（3 桁数値）を付加する。**最後の**
/// sentinel 以降を status トレーラとして切り出す（本文に sentinel 同一文字列が現れる確率は無視できるが、万一
/// 現れても最後の出現＝curl が付けたトレーラを採る）。sentinel が見つからない想定外出力は status `0`・本文
/// 全体とし、呼び出し側が空振り扱いで `None` へ縮退する。token はこの出力に現れない（status は数値のみ）。
fn split_status(raw: &str) -> (u16, String) {
    let Some(sentinel_at) = raw.rfind(RELEASES_META_SENTINEL) else {
        return (0, raw.to_string());
    };
    let body = raw[..sentinel_at].to_string();
    let status_text = &raw[sentinel_at + RELEASES_META_SENTINEL.len()..];
    let status = status_text.trim().parse::<u16>().unwrap_or(0);
    (status, body)
}

/// Releases 一覧取得用の curl 引数列を組み立てる純粋関数。
///
/// host allowlist 契約（redirect 不追従・https 限定）は support の安全 fetch 引数列
/// （[`safe_fetch_args`](crate::update_history::support::safe_fetch_args)）と同一。`Accept`/`X-GitHub-Api-Version`
/// で GitHub REST API の JSON 応答を固定する。`with_auth` のとき `--config -`（stdin）から Authorization を
/// 読むため `--config -` を加える（token 本体は argv に置かず stdin で渡す＝secret 非露出）。token を含まない
/// ヘッダ（`Accept` 等）は argv に置いてよい。
///
/// 観測: `--fail` は付けない。4xx/5xx を curl exit へ倒すと本物の HTTP status を握り潰し、認証/レート失敗
/// （401/403/429）を空振りと区別できなくなる（サイレント全滅）。代わりに `--write-out` で本文末尾へ
/// [`RELEASES_META_SENTINEL`] + `%{http_code}` を付加し、[`fetch_releases_page`] が status を読んで診断する。
/// `%{http_code}` は数値のみで token を含まない。
fn releases_list_curl_args(url: &str, with_auth: bool) -> Vec<OsString> {
    let mut args = Vec::with_capacity(16);
    if with_auth {
        // token は stdin（--config -）の Authorization ヘッダで渡す（argv 非露出）。
        args.push(OsString::from("--config"));
        args.push(OsString::from("-"));
    }
    args.extend([
        OsString::from("--silent"),
        OsString::from("--show-error"),
        // redirect を追従しない（allowlist 外 host への横滑りを塞ぐ）。
        OsString::from("--max-redirs"),
        OsString::from("0"),
        OsString::from("--proto"),
        OsString::from("=https"),
        // 応答しない host で record job が止まらないよう接続/転送全体を有界化する（finding 3368730838）。
        OsString::from("--connect-timeout"),
        OsString::from(FETCH_CONNECT_TIMEOUT_SECS),
        OsString::from("--max-time"),
        OsString::from(FETCH_MAX_TIME_SECS),
        // GitHub REST API の JSON 応答を固定する（secret 非含有のため argv 可）。
        OsString::from("--header"),
        OsString::from("Accept: application/vnd.github+json"),
        OsString::from("--header"),
        OsString::from("X-GitHub-Api-Version: 2022-11-28"),
        // 本文末尾へ sentinel + HTTP status を付加する（token は含まれない＝数値のみ）。認証/レート失敗を
        // 空振りと区別するため `--fail` を使わず status を読む。
        OsString::from("--write-out"),
        OsString::from(format!("{RELEASES_META_SENTINEL}%{{http_code}}")),
        OsString::from(url),
    ]);
    args
}

/// curl の `--config -`（stdin）へ流す設定行を組み立てる。token を argv に出さず Authorization ヘッダを渡す。
///
/// curl 設定ファイル構文の `header = "..."` で Authorization ヘッダ 1 件だけを与える。値はダブルクォートで
/// 囲み、token 内に万一含まれうる `\` と `"` をエスケープして構文を壊さない。この文字列は stdin 経由でのみ
/// curl へ渡り、argv・ログには現れない。
fn auth_config(token: &str) -> String {
    let escaped = token.replace('\\', "\\\\").replace('"', "\\\"");
    format!("header = \"Authorization: Bearer {escaped}\"\n")
}

/// Releases 一覧 JSON（リリース配列）から `(tag_name, name, body)` を持つ [`Release`] 列へ翻訳する純粋関数。
///
/// 応答が JSON 配列でないときは `None`。各要素から `tag_name`/`name`/`body`（いずれも文字列。欠落・非文字列は
/// 空）を取り出す。draft/prerelease の除外は行わない（リリースノートが書かれていれば材料にする）。
fn parse_releases(json: &str) -> Option<Vec<Release>> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let array = value.as_array()?;
    let releases = array
        .iter()
        .map(|item| Release {
            tag_name: item
                .get("tag_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: item
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            body: item
                .get("body")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
        .collect();
    Some(releases)
}

/// Releases API の 1 リリース（範囲フィルタ・連結に必要な最小フィールド）。
struct Release {
    /// リリースの tag（`v1.2.3` / `1.2.3` / `<name>-v1.2.3` 等の揺れがある）。
    tag_name: String,
    /// リリース表示名（tag が空のとき version 抽出のフォールバックに使う）。
    name: String,
    /// リリースノート markdown 本文（信頼境界外）。空のこともある。
    body: String,
}

impl Release {
    /// `(old, new]` 範囲に入りかつ本文がある場合、整列キーとなる version を返す（domain 規則へ委譲）。
    ///
    /// 範囲判定（old 排他・new 包含）と tag→version 正規化は domain rule（[`version_in_range`] /
    /// [`release_version`]）であり、adapter はそれを Releases API JSON へ適用するだけである。本文が空
    /// （リリースノート無し）・version 抽出不能・範囲外はいずれも `None`（連結対象から除外）。返す version は
    /// 古い順整列（[`join_release_bodies`]）のキーに使う。
    fn in_range_version(&self, old: Option<&str>, new: Option<&str>) -> Option<String> {
        if self.body.trim().is_empty() {
            return None;
        }
        let version = release_version(&self.tag_name, &self.name)?;
        version_in_range(&version, old, new).then_some(version)
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

/// brew cask `.rb` 定義テキストから探索ヒント URL（`homepage` 優先、無ければ `url`）を 1 件抽出する純粋関数。
///
/// cask 定義は Ruby DSL で `homepage "https://..."` / `url "https://..."` の行を持つ（finding 3374863454）。
/// これらは実リリースノート本文ではないため seed にせず、homepage（プロジェクト公式サイト＝ノート探索の
/// 起点として最適）を一次、`url`（配布物 URL）をフォールバックにして AI tool-use 探索のヒント host を得る。
/// 行頭の空白を許し、`homepage`/`url` キーワードに続く最初の **ダブルクォート文字列**の中身を取り出す。
/// 抽出できなければ `None`（探索ヒント無し＝version-only へ縮退）。値は信頼境界外であり、後段の host
/// allowlist / agent_loop 側 SSRF 検査で守る（ここでは形の抽出だけを行う）。
fn parse_cask_hint(rb: &str) -> Option<String> {
    extract_dsl_string(rb, "homepage").or_else(|| extract_dsl_string(rb, "url"))
}

/// cask `.rb` の Ruby DSL から `<key> "<value>"` の最初の二重引用符文字列値を抽出する純粋関数。
///
/// 各行を走査し、trim 後に `<key>` で始まりその直後が空白の行を対象にして、最初の `"` と次の `"` の間を値と
/// する。`key` 直後が空白でない行（`url_template` 等の別キー）は対象にしない。値が空、または引用符が閉じない
/// 行は対象外として次行を見る。最初に得た非空値を返す（無ければ `None`）。
fn extract_dsl_string(rb: &str, key: &str) -> Option<String> {
    for line in rb.lines() {
        let trimmed = line.trim_start();
        let Some(after_key) = trimmed.strip_prefix(key) else {
            continue;
        };
        // `key` 直後は空白でなければならない（`url_template` のような別キーを誤検出しない）。
        if !after_key.starts_with(char::is_whitespace) {
            continue;
        }
        let Some(open) = after_key.find('"') else {
            continue;
        };
        let rest = &after_key[open + 1..];
        let Some(close) = rest.find('"') else {
            continue;
        };
        let value = &rest[..close];
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
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
    //! および Releases API curl の redirect 不追従引数列（host allowlist 契約をコードで保証）を固定する。
    //! 安全 fetch 引数列（`-L` 無し）の退行固定は support module の test が担う（curl 経路の一本化）。

    use super::{
        NotesFetchPlan, Release, ReleaseNotesAdapter, auth_config, extract_release_body,
        join_release_bodies, parse_cask_hint, parse_releases, release_api_curl_args,
        releases_list_curl_args, releases_list_url, releases_page_url, resolve_nix_notes_source,
        resolve_notes_url, split_owner_repo, split_status,
    };
    use crate::update_history::domain::diff::DeltaSource;
    use crate::update_history::ports::NotesPort;

    /// curl 引数列が接続上限（`--connect-timeout`）と転送全体上限（`--max-time`）を有界値で含むことを固定する
    /// 共通アサーション（finding 3368730838 の退行固定）。各 flag の直後に空でない数値が続くことを検証する
    /// （数値の正確値は定数側の責務、ここでは有界 timeout が「存在し数値で続く」ことだけを退行固定する）。
    fn assert_bound_timeout_flags(args: &[String]) {
        for flag in ["--connect-timeout", "--max-time"] {
            let idx = args.iter().position(|arg| arg == flag);
            assert!(idx.is_some(), "{flag} を指定する: {args:?}");
            let value = idx.and_then(|i| args.get(i + 1)).map(String::as_str);
            assert!(
                value.is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())),
                "{flag} の直後に有界な数値が続く: {args:?}"
            );
        }
    }

    #[test]
    fn nix_without_repo_or_notes_source_degrades_to_none_without_network() -> crate::Result<()> {
        // nix eval 由来 package の repo・notes_source がともに不明（None）または空ならノート無しへ縮退し、
        // curl を一切起動しない（hermetic: network 非依存）。repo 無し＝Releases API を試みず、notes_source
        // 無し＝changelog フォールバックも無いため即縮退する。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(
            adapter
                .fetch_release_notes("neovim", DeltaSource::NixEval, None, None, None, None)?
                .is_none()
        );
        // repo/notes_source が空白だけでも縮退（trim して空判定）。
        assert!(
            adapter
                .fetch_release_notes(
                    "neovim",
                    DeltaSource::NixEval,
                    Some("   ".to_string()),
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
        // `resolve_nix_notes_source` が github.com 以外を弾いて `None` を返し、curl を踏まず縮退する。
        // repo 無しなので Releases API も試みない（hermetic: network 非依存）。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(
            adapter
                .fetch_release_notes(
                    "evilpkg",
                    DeltaSource::NixEval,
                    None,
                    Some("https://evil.example/changelog".to_string()),
                    None,
                    None,
                )?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn nix_with_invalid_repo_falls_back_to_changelog_resolution() -> crate::Result<()> {
        // repo が owner/repo 形でない（split_owner_repo が None）なら Releases API を試みず、changelog
        // フォールバックへ倒す。changelog も repo root（変換不能）なら最終的に None（hermetic）。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(
            adapter
                .fetch_release_notes(
                    "weird",
                    DeltaSource::NixEval,
                    Some("not-a-valid-repo".to_string()),
                    Some("https://github.com/o/r".to_string()),
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
                .fetch_release_notes("firefox", DeltaSource::BrewTap, None, None, None, None)?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn brew_fetch_release_notes_never_seeds_with_cask_definition() -> crate::Result<()> {
        // finding 3374863454 退行固定: brew tap 由来の `fetch_release_notes` は cask `.rb` 定義そのものを
        // seed（summarize_only 経路）にしない。base が設定されていても seed は常に `None` を返す
        // （定義ファイルを直接要約しない）。旧実装は `<base><letter>/<name>.rb` を fetch して `.rb` 本文を
        // `RawReleaseNotes` で返し、非空 seed として summarize_only に入っていた。base 有無に依らず seed=None を
        // 固定する（curl を起動せず hermetic）。
        let base = "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/";
        let adapter = ReleaseNotesAdapter::new(Some(base.to_string()));
        assert!(
            adapter
                .fetch_release_notes("firefox", DeltaSource::BrewTap, None, None, None, None)?
                .is_none(),
            "cask `.rb` 定義は seed にせず None を返す（summarize_only に入らない）"
        );
        Ok(())
    }

    #[test]
    fn brew_notes_hint_degrades_to_none_without_base() -> crate::Result<()> {
        // base 未指定なら探索ヒントも解決できず None（curl を起動しない hermetic）。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(adapter.resolve_brew_notes_hint("firefox")?.is_none());
        Ok(())
    }

    #[test]
    fn parse_cask_hint_prefers_homepage_then_url() {
        // finding 3374863454: cask `.rb` 定義から探索ヒントを取り出す。homepage を一次、url をフォールバックに。
        let rb = "cask \"firefox\" do\n  version \"121.0\"\n  url \"https://download.example/firefox.dmg\"\n  homepage \"https://www.mozilla.org/firefox/\"\nend\n";
        assert_eq!(
            parse_cask_hint(rb).as_deref(),
            Some("https://www.mozilla.org/firefox/")
        );
        // homepage が無ければ url をヒントにする。
        let rb_no_home =
            "cask \"x\" do\n  url \"https://github.com/o/r/releases/download/v1/x.zip\"\nend\n";
        assert_eq!(
            parse_cask_hint(rb_no_home).as_deref(),
            Some("https://github.com/o/r/releases/download/v1/x.zip")
        );
        // homepage/url が無ければヒント無し。`url_template` のような別キーは誤検出しない。
        let rb_none = "cask \"x\" do\n  url_template \"https://example/#{version}\"\nend\n";
        assert!(parse_cask_hint(rb_none).is_none());
        assert!(parse_cask_hint("cask \"x\" do\nend\n").is_none());
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
        // 退行固定（finding 3368730838）: Releases API 取得 curl も接続/転送全体を有界化し、応答しない host で
        // record job が子プロセス完了待ちのまま timeout まで止まらないようにする。
        assert_bound_timeout_flags(&args);
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
        // nix 経路で repo 無し・notes_source が repo root（変換不能）なら curl を起動せず None へ縮退する
        // （hermetic）。repo 無し＝Releases API を試みず、changelog も repo root で変換不能のため即縮退。
        let adapter = ReleaseNotesAdapter::new(None);
        assert!(
            adapter
                .fetch_release_notes(
                    "neovim",
                    DeltaSource::NixEval,
                    None,
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

    // --- Releases API 範囲取得まわりの純粋部分（実 network 非依存・fixture） ---

    #[test]
    fn split_owner_repo_accepts_exactly_one_slash() {
        assert_eq!(
            split_owner_repo("neovim/neovim"),
            Some(("neovim", "neovim"))
        );
        assert_eq!(
            split_owner_repo("BurntSushi/ripgrep"),
            Some(("BurntSushi", "ripgrep"))
        );
        // 不正形（スラッシュ無し・過多・端のスラッシュ）は None（Releases API を試みず changelog へ倒す）。
        assert_eq!(split_owner_repo("noslash"), None);
        assert_eq!(split_owner_repo("a/b/c"), None);
        assert_eq!(split_owner_repo("/repo"), None);
        assert_eq!(split_owner_repo("owner/"), None);
        assert_eq!(split_owner_repo(""), None);
    }

    #[test]
    fn releases_list_url_and_page_url_are_well_formed() {
        // API URL は allowlist 済み host（api.github.com）で per_page/page クエリを持つ。
        assert_eq!(
            releases_list_url("o", "r", 1),
            "https://api.github.com/repos/o/r/releases?per_page=100&page=1"
        );
        // 記録用 URL は人間が辿れる releases ページ（github.com）。
        assert_eq!(
            releases_page_url("o", "r"),
            "https://github.com/o/r/releases"
        );
    }

    #[test]
    fn auth_config_puts_token_in_stdin_header_not_argv() {
        // token は curl の `--config -`（stdin）の Authorization ヘッダとして渡し、argv には出さない。
        assert_eq!(
            auth_config("ghs_SECRET123"),
            "header = \"Authorization: Bearer ghs_SECRET123\"\n"
        );
        // 構文を壊しうる文字（`\`・`"`）はエスケープする。
        assert_eq!(
            auth_config(r#"a\b"c"#),
            "header = \"Authorization: Bearer a\\\\b\\\"c\"\n"
        );
    }

    #[test]
    fn releases_list_curl_args_omit_token_from_argv_and_set_no_redirects() {
        // with_auth=true でも token 本体は argv に乗らず、`--config -`（stdin 読み）だけが付く。
        let args: Vec<String> = releases_list_curl_args(
            "https://api.github.com/repos/o/r/releases?per_page=100&page=1",
            true,
        )
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
        // token 文字列は argv のどこにも現れない（stdin config で渡すため）。
        assert!(
            !args
                .iter()
                .any(|a| a.contains("Bearer") || a.contains("Authorization")),
            "argv に Authorization/token を載せてはならない: {args:?}"
        );
        // stdin config 読みを指示する。
        assert!(args.windows(2).any(|w| w[0] == "--config" && w[1] == "-"));
        // redirect 不追従・https 限定（host allowlist 契約）。
        assert!(!args.iter().any(|a| a == "--location" || a == "-L"));
        let mr = args
            .iter()
            .position(|a| a == "--max-redirs")
            .expect("--max-redirs");
        assert_eq!(args.get(mr + 1).map(String::as_str), Some("0"));
        let proto = args.iter().position(|a| a == "--proto").expect("--proto");
        assert_eq!(args.get(proto + 1).map(String::as_str), Some("=https"));
        // 退行固定（finding 3368730838）: auth 経路でも接続/転送全体を有界化する。
        assert_bound_timeout_flags(&args);
        // 取得対象 URL が末尾に乗る。
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://api.github.com/repos/o/r/releases?per_page=100&page=1")
        );
    }

    #[test]
    fn releases_list_curl_args_without_auth_omit_config() {
        // 未認証（token 無し）のときは `--config -` を付けない。
        let args: Vec<String> =
            releases_list_curl_args("https://api.github.com/repos/o/r/releases", false)
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
        assert!(!args.iter().any(|a| a == "--config"));
        // `--fail` は付けない（4xx/5xx を curl exit へ倒すと認証/レート失敗を空振りと区別できなくなる）。
        assert!(!args.iter().any(|a| a == "--fail"));
        // 代わりに `--write-out` で本文末尾へ status を付加して読む。
        assert!(args.iter().any(|a| a == "--write-out"));
        // 退行固定（finding 3368730838）: 非 auth 経路でも接続/転送全体を有界化する。
        assert_bound_timeout_flags(&args);
    }

    #[test]
    fn releases_list_curl_args_write_out_appends_status_sentinel() {
        // 観測: `--write-out` の値が sentinel + `%{http_code}` で、HTTP status を本文末尾へ付加する。
        let args: Vec<String> =
            releases_list_curl_args("https://api.github.com/repos/o/r/releases", true)
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
        let wo = args
            .iter()
            .position(|a| a == "--write-out")
            .expect("--write-out");
        let value = args
            .get(wo + 1)
            .map(String::as_str)
            .expect("write-out value");
        assert!(value.ends_with("%{http_code}"), "{value:?}");
        assert!(value.contains(super::RELEASES_META_SENTINEL), "{value:?}");
    }

    #[test]
    fn split_status_separates_body_and_http_code() {
        // curl 出力（本文 + sentinel + status）から status と本文を切り出す。
        let raw = format!(
            "{body}{sentinel}403",
            body = r#"{"message":"rate limited"}"#,
            sentinel = super::RELEASES_META_SENTINEL
        );
        let (status, body) = split_status(&raw);
        assert_eq!(status, 403);
        assert_eq!(body, r#"{"message":"rate limited"}"#);

        // 200 本文も同様に切り出せる。
        let raw = format!("[]{sentinel}200", sentinel = super::RELEASES_META_SENTINEL);
        let (status, body) = split_status(&raw);
        assert_eq!(status, 200);
        assert_eq!(body, "[]");
    }

    #[test]
    fn split_status_returns_zero_when_no_sentinel() {
        // 想定外出力（sentinel 無し）は status 0・本文全体（呼び出し側で空振り扱い→None 縮退）。
        let (status, body) = split_status("no sentinel here");
        assert_eq!(status, 0);
        assert_eq!(body, "no sentinel here");
    }

    /// fixture: tag_name/name/body を持つ releases 配列 JSON を組む。
    fn releases_json(items: &[(&str, &str, &str)]) -> String {
        let array: Vec<serde_json::Value> = items
            .iter()
            .map(|(tag, name, body)| {
                serde_json::json!({ "tag_name": tag, "name": name, "body": body })
            })
            .collect();
        serde_json::Value::Array(array).to_string()
    }

    #[test]
    fn parse_releases_reads_array_of_tag_name_body() {
        let json = releases_json(&[("v1.0.0", "1.0.0", "first"), ("v1.1.0", "", "second")]);
        let releases = parse_releases(&json).expect("array");
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag_name, "v1.0.0");
        assert_eq!(releases[0].body, "first");
        assert_eq!(releases[1].tag_name, "v1.1.0");
    }

    #[test]
    fn parse_releases_returns_none_for_non_array() {
        assert!(parse_releases(r#"{"message":"Not Found"}"#).is_none());
        assert!(parse_releases("not json").is_none());
    }

    fn release(tag: &str, body: &str) -> Release {
        Release {
            tag_name: tag.to_string(),
            name: String::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn in_range_version_applies_domain_range_and_returns_sort_key() {
        // 範囲判定（old 排他・new 包含）は domain へ委譲し、範囲内なら整列キー version を返す。
        let old = Some("1.0.0");
        let new = Some("2.0.0");
        // tag 揺れ（`v` 接頭・接頭辞付き・接頭なし）でも domain の version 抽出で照合し、正規化 version を返す。
        assert_eq!(
            release("v1.5.0", "x").in_range_version(old, new).as_deref(),
            Some("1.5.0")
        );
        assert_eq!(
            release("mypkg-v1.5.0", "x")
                .in_range_version(old, new)
                .as_deref(),
            Some("1.5.0")
        );
        // old 排他・new 超過は範囲外（None）。
        assert!(release("v1.0.0", "x").in_range_version(old, new).is_none());
        assert!(release("v2.5.0", "x").in_range_version(old, new).is_none());
    }

    #[test]
    fn in_range_version_excludes_empty_body_and_unextractable() {
        let old = Some("1.0.0");
        let new = Some("2.0.0");
        // body 空（リリースノート無し）は範囲内でも除外（LLM の材料にならない）。これは adapter 側の翻訳判断
        // （JSON `.body` の有無）であり domain 範囲判定の前に弾く。
        assert!(
            release("v1.5.0", "   ")
                .in_range_version(old, new)
                .is_none()
        );
        // version を抽出できない tag/name は除外（domain の release_version が None）。
        assert!(release("latest", "x").in_range_version(old, new).is_none());
    }

    #[test]
    fn join_release_bodies_sorts_ascending_and_joins_with_separator() {
        // 退行固定: 複数 in-range body を version 昇順（古い順）に整列し、`\n\n---\n\n` で連結する。
        // 入力順は新しい順（Releases API の返却順）でも、version で安定整列して古い順にする。
        let bodies = vec![
            ("2.0.0".to_string(), "new body".to_string()),
            ("1.0.0".to_string(), "old body".to_string()),
            ("1.5.0".to_string(), "mid body".to_string()),
        ];
        assert_eq!(
            join_release_bodies(bodies),
            "old body\n\n---\n\nmid body\n\n---\n\nnew body"
        );
        // 空入力は空文字（呼び出し側が None 縮退に倒す）。
        assert_eq!(join_release_bodies(Vec::new()), "");
        // 1 件は区切り無しでそのまま。
        assert_eq!(
            join_release_bodies(vec![("1.0.0".to_string(), "only".to_string())]),
            "only"
        );
    }

    #[test]
    fn join_release_bodies_orders_two_digit_versions_numerically() {
        // 退行固定: 整列は domain の version_ordering（成分単位比較）へ委譲する。`String` 字句比較だと
        // `"10.0.0"` が `"2.0.0"`/`"9.0.0"` より前に来て二桁 version で連結順が逆転する。成分比較なら
        // 昇順は 2.0.0 < 9.0.0 < 10.0.0 になる。入力順は新しい順（Releases API の返却順）。
        let bodies = vec![
            ("10.0.0".to_string(), "ten".to_string()),
            ("9.0.0".to_string(), "nine".to_string()),
            ("2.0.0".to_string(), "two".to_string()),
        ];
        assert_eq!(
            join_release_bodies(bodies),
            "two\n\n---\n\nnine\n\n---\n\nten"
        );
    }
}

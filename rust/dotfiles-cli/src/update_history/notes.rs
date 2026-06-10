//! リリースノートの機械取得（GitHub Releases API / changelog raw / cask 探索ヒント）・版差分入力の読み取り・
//! 安全 HTTP fetch primitive。
//!
//! version 差分入力（nix eval JSON・brew tap 版差分ファイル）の読み取りと、各パッケージの生リリースノート取得を
//! 担う。取得は外部 `curl` を起動せず、本 module 内の `reqwest`（blocking）client へ集約する。SSRF 対策として
//! client を **redirect 不追従**・**https 限定**・**有界 timeout**で構成し、本文は読み取り上限で打ち切る。host
//! allowlist 検査は呼び出し側（[`super::wire::is_allowed_url`]）の責務で、この層は host を再判定しない。本文は
//! 信頼境界外（prompt injection 源）のまま返し、構造化・要約は LLM（[`super::llm`]）の責務。
//!
//! ノート取得は出所で分かれる:
//! - **nix eval 由来**: delta が運ぶ `repo`（GitHub Releases API で `(old, new]` 範囲を取得）を一次に、空振り時は
//!   `notes_source`（changelog blob→raw / releases/tag→Releases API `.body`）へフォールバック。両方不能なら `None`。
//! - **brew tap 由来**: cask `.rb` 定義は実ノート本文でないため seed にしない。`brew_notes_hint` が cask 定義から
//!   homepage/url を探索ヒントとして取り出し、AI tool-use 探索へ回す。

use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use super::diff::{
    DeltaSource, NixPackage, VersionDelta, release_version, version_in_range, version_ordering,
};
use super::wire::{host_of, is_allowed_url};
use crate::Result;

/// 接続確立の上限。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// リクエスト全体（接続〜本文読み取り）の上限。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// 1 レスポンス本文の読み取り上限（バイト）。超過分は読まずに打ち切る。
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

/// GitHub ホストへの GET をレート/一過性失敗時に再試行する最大回数（初回を除く追加試行数）。
///
/// GitHub 以外のホストは [`NON_GITHUB_MAX_RETRIES`] に絞り、総待機が record job の `timeout-minutes:120` を
/// 圧迫しないようにする。
const GITHUB_MAX_RETRIES: u32 = 3;
/// GitHub 以外のホストへの GET の追加試行数（接続失敗の取りこぼし防止に 1 回だけ。レート制限は GitHub 固有）。
const NON_GITHUB_MAX_RETRIES: u32 = 1;
/// 指数バックオフの初期待機（秒）。`base * 2^attempt` で増やす。
const BACKOFF_BASE_SECS: u64 = 1;
/// 1 回のバックオフ待機の上限（秒）。`Retry-After` も指数項もこの値で頭打ちにする。
const BACKOFF_MAX_SECS: u64 = 30;
/// 1 リクエストあたりのバックオフ総待機の上限（秒）。これを超える待機が必要なら諦めて縮退する。
///
/// 最悪ケース見積もり（GitHub host・全試行がバックオフ対象）: attempt0=1s, attempt1=2s, attempt2=4s の合計 7s。
/// 53 パッケージが各複数 fetch（releases ページ最大 3 + フォールバック + AI fetch）でも、認証付与により
/// レート枯渇は起きにくく、最悪でも 1 パッケージ数十秒・全体は 120 分の timeout に十分収まる。
const BACKOFF_TOTAL_CAP_SECS: u64 = 60;

/// 共有 blocking client（redirect 不追従・https 限定・有界 timeout）。初回アクセスで 1 度だけ構築する。
static HTTP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

/// 全 HTTP 取得に添える固定 User-Agent。GitHub REST API は UA 無しのリクエストを 403 で拒否するため、共有
/// client の default header として 1 箇所で付与し、GitHub API・raw.githubusercontent.com 双方の経路に効かせる。
const HTTP_USER_AGENT: &str = concat!("dotfiles-update-history/", env!("CARGO_PKG_VERSION"));

/// 1 リクエストに添える追加ヘッダ（名前と値の組）。
type Header<'a> = (&'a str, &'a str);

/// HTTP GET の最小レスポンス（status と上限付き本文）。
struct HttpResponse {
    status: u16,
    body: String,
}

/// 1 回の GET 試行の結果（再試行判定・バックオフ算出に必要な最小情報）。
///
/// `Connected` は応答が返った（status/body と再試行に効くヘッダを保持）。`SendError` は接続/転送段の失敗。
enum Attempt {
    Connected {
        status: u16,
        body: String,
        /// `Retry-After` ヘッダ（秒数 or HTTP-date 文字列。あれば）。
        retry_after: Option<String>,
        /// GitHub の `X-RateLimit-Remaining`（あれば。`0` は primary rate limit 枯渇の兆候）。
        rate_remaining: Option<String>,
    },
    SendError,
}

/// 共有 client を取得する（構築失敗時は都度新規構築へフォールバックして握り潰さない）。
fn http_client() -> Result<&'static reqwest::blocking::Client> {
    if let Some(client) = HTTP_CLIENT.get() {
        return Ok(client);
    }
    let built = build_http_client()?;
    Ok(HTTP_CLIENT.get_or_init(|| built))
}

fn build_http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .user_agent(HTTP_USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()?)
}

/// 許可済み https URL を redirect 不追従・有界本文で GET する（host 検査は呼び出し側責務）。
///
/// GitHub 系ホスト（[`is_github_host`]）には `GITHUB_TOKEN` を `Authorization: Bearer` で添え、未認証
/// 60req/hr ではなく認証 5000req/hr の枠で叩く（token は reqwest のヘッダ値として渡すため argv/URL/ログに
/// 出ない）。GitHub 以外のホストには token を**絶対に付けない**（漏えい防止）。レート/一過性失敗
/// （403 secondary・429・5xx・接続失敗）は有界バックオフで少数回再試行する（[`retry_decision`]）。
///
/// 取得成功は `Some(HttpResponse)`、接続/転送失敗・再試行枯渇は `None`（呼び出し側が縮退する）。本文は
/// [`MAX_RESPONSE_BYTES`] までで打ち切って読む。
fn http_get(url: &str, headers: &[Header<'_>]) -> Result<Option<HttpResponse>> {
    let github_host = host_of(url).is_some_and(|host| is_github_host(&host));
    // GitHub 系ホストにだけ token を添える（host 一致時のみ。token 漏えい防止）。
    let authorization = if github_host {
        github_token().map(|token| format!("Bearer {token}"))
    } else {
        None
    };
    let max_retries = if github_host {
        GITHUB_MAX_RETRIES
    } else {
        NON_GITHUB_MAX_RETRIES
    };

    let mut waited_total: u64 = 0;
    let mut attempt_index: u32 = 0;
    loop {
        let attempt = http_get_once(url, headers, authorization.as_deref())?;
        let decision = retry_decision(&attempt, github_host);
        if let RetryDecision::Done(response) = decision {
            return Ok(response);
        }
        if attempt_index >= max_retries {
            return Ok(attempt_to_response(&attempt));
        }
        let retry_after = match &attempt {
            Attempt::Connected { retry_after, .. } => retry_after.as_deref(),
            Attempt::SendError => None,
        };
        let Some(wait) = backoff_wait_secs(
            attempt_index,
            retry_after,
            waited_total,
            BACKOFF_TOTAL_CAP_SECS,
        ) else {
            // これ以上待つと総待機上限を超える → 諦めて縮退する。
            return Ok(attempt_to_response(&attempt));
        };
        if wait > 0 {
            std::thread::sleep(Duration::from_secs(wait));
        }
        waited_total = waited_total.saturating_add(wait);
        attempt_index += 1;
    }
}

/// 1 回だけ GET を試み、再試行判定に必要な情報を [`Attempt`] へ翻訳する。
fn http_get_once(
    url: &str,
    headers: &[Header<'_>],
    authorization: Option<&str>,
) -> Result<Attempt> {
    let client = http_client()?;
    let mut request = client.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if let Some(authorization) = authorization {
        request = request.header("Authorization", authorization);
    }
    let response = match request.send() {
        Ok(response) => response,
        Err(_) => return Ok(Attempt::SendError),
    };
    let status = response.status().as_u16();
    let retry_after = header_value(response.headers(), "retry-after");
    let rate_remaining = header_value(response.headers(), "x-ratelimit-remaining");
    let body = read_capped(response, MAX_RESPONSE_BYTES);
    Ok(Attempt::Connected {
        status,
        body,
        retry_after,
        rate_remaining,
    })
}

/// レスポンスヘッダから 1 値を ASCII 文字列として取り出す（非 ASCII/不在は `None`）。
fn header_value(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// 再試行の判定結果。`Done` は確定（`Some(response)`=応答あり / `None`=接続失敗で縮退）、`Retry` は再試行対象。
enum RetryDecision {
    Done(Option<HttpResponse>),
    Retry,
}

/// 1 回の試行結果を「確定（縮退含む）か再試行対象か」へ翻訳する純粋関数（network 抜きで決定論的）。
///
/// 再試行対象: 接続失敗（`SendError`）・429・5xx・403（secondary rate limit 兆候があるもの）。即確定:
/// 2xx・404 等それ以外。primary rate limit（`X-RateLimit-Remaining: 0`）の 403 は reset まで待つと長すぎる
/// ため**再試行せず縮退**する（認証付与で primary 枯渇は起きにくい前提）。GitHub 以外のホストは 403 の
/// secondary 判定を行わず（rate limit 概念は GitHub 固有）429/5xx/接続失敗のみ再試行対象にする。
fn retry_decision(attempt: &Attempt, github_host: bool) -> RetryDecision {
    match attempt {
        Attempt::SendError => RetryDecision::Retry,
        Attempt::Connected {
            status,
            body,
            rate_remaining,
            ..
        } => {
            if *status == 429 || (500..600).contains(status) {
                return RetryDecision::Retry;
            }
            if *status == 403 && github_host {
                // primary rate limit（remaining=0）は reset 待ちが長すぎるため再試行せず縮退する。
                if is_primary_rate_limited(rate_remaining.as_deref()) {
                    return RetryDecision::Done(None);
                }
                if is_secondary_rate_limited(body) {
                    return RetryDecision::Retry;
                }
            }
            RetryDecision::Done(attempt_to_response(attempt))
        }
    }
}

/// `X-RateLimit-Remaining` が `0` なら primary rate limit 枯渇とみなす純粋判定。
fn is_primary_rate_limited(rate_remaining: Option<&str>) -> bool {
    rate_remaining.map(str::trim) == Some("0")
}

/// 403 本文が GitHub の secondary rate limit / abuse 検知の兆候を含むかの純粋判定（小文字化して部分一致）。
fn is_secondary_rate_limited(body: &str) -> bool {
    let lowered = body.to_ascii_lowercase();
    lowered.contains("secondary rate limit")
        || lowered.contains("abuse detection")
        || lowered.contains("rate limit")
}

/// [`Attempt`] を呼び出し側の [`HttpResponse`] へ翻訳する（接続失敗は `None`。再試行枯渇/上限到達時の縮退に使う）。
fn attempt_to_response(attempt: &Attempt) -> Option<HttpResponse> {
    match attempt {
        Attempt::SendError => None,
        Attempt::Connected { status, body, .. } => Some(HttpResponse {
            status: *status,
            body: body.clone(),
        }),
    }
}

/// 指定したホストが GitHub 系（token を添えてよい・rate limit 再試行の対象）かの純粋判定。
///
/// `host_of` で正規化済み小文字 host を前提に、`github.com` / `api.github.com` / `raw.githubusercontent.com`
/// と、その厳密なサブドメイン（`.github.com` 等で終わる）だけを GitHub 系とみなす。`notgithub.com` のような
/// 接尾辞偽装は弾く（`ends_with(".github.com")` の先頭ドット必須）。
fn is_github_host(host: &str) -> bool {
    const GITHUB_HOSTS: [&str; 3] = ["github.com", "api.github.com", "raw.githubusercontent.com"];
    GITHUB_HOSTS
        .iter()
        .any(|root| host == *root || host.ends_with(&format!(".{root}")))
}

/// 次回バックオフの待機秒を算出する純粋関数（`Retry-After` 優先、無ければ指数）。
///
/// `attempt_index` は 0 始まり（初回失敗後の待機が index=0）。`Retry-After` が秒数としてパースできればそれを、
/// できなければ `BACKOFF_BASE_SECS * 2^attempt_index` を採り、いずれも [`BACKOFF_MAX_SECS`] で頭打ちにする。
/// `waited_total + wait` が `total_cap` を超える場合は `None`（これ以上待たず諦める）。
fn backoff_wait_secs(
    attempt_index: u32,
    retry_after: Option<&str>,
    waited_total: u64,
    total_cap: u64,
) -> Option<u64> {
    let wait = match retry_after.and_then(parse_retry_after_secs) {
        Some(secs) => secs.min(BACKOFF_MAX_SECS),
        None => {
            let factor = 1u64.checked_shl(attempt_index).unwrap_or(u64::MAX);
            BACKOFF_BASE_SECS
                .saturating_mul(factor)
                .min(BACKOFF_MAX_SECS)
        }
    };
    if waited_total.saturating_add(wait) > total_cap {
        return None;
    }
    Some(wait)
}

/// `Retry-After` ヘッダ値を秒数として解釈する純粋関数（delta-seconds 形式のみ。HTTP-date は対象外で `None`）。
fn parse_retry_after_secs(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

/// 任意の [`Read`] を `limit` バイトまで読み、UTF-8 として lossy にデコードする純粋規約。
///
/// `take` で読み取り段階から上限を掛け、巨大本文を全量バッファしない（資源枯渇防止）。読み取り失敗は読めた分だけ
/// 返す（接続途中切断でも部分本文を活かす）。
fn read_capped<R: Read>(reader: R, limit: u64) -> String {
    let mut buffer = Vec::new();
    let mut limited = reader.take(limit);
    let _ = limited.read_to_end(&mut buffer);
    String::from_utf8_lossy(&buffer).into_owned()
}

/// GitHub Releases API のページサイズ（API 上限は 100）。
const RELEASES_PER_PAGE: u32 = 100;
/// Releases API のページング取得上限ページ数。
const MAX_RELEASE_PAGES: u32 = 3;
/// 範囲取得した複数リリース `.body` を連結する区切り（古い順に積む）。
const RELEASE_BODY_SEPARATOR: &str = "\n\n---\n\n";

/// 取得済み生リリースノートと参照 URL の境界型。
///
/// `text` は信頼境界外の生テキスト。`notes_url` は記録・表示に残すノート参照 URL。`refetch_url` は同じ本文を
/// raw 取得し直せる URL（あれば。provenance の再利用 source に学習してよいのはこれだけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawReleaseNotes {
    pub(crate) text: String,
    pub(crate) notes_url: String,
    pub(crate) refetch_url: Option<String>,
}

/// 与えた https URL を redirect 不追従・https 限定の reqwest で取得し、2xx の非空本文だけを `Some` で返す。
///
/// host allowlist 検査は呼び出し側の責務（この primitive は host を再判定しない）。取得失敗・非 2xx・空本文は
/// `None`（curl の `--fail` 相当を status で判定する）。
pub(crate) fn safe_https_fetch(url: &str) -> Result<Option<String>> {
    let Some(response) = http_get(url, &[])? else {
        return Ok(None);
    };
    if (200..300).contains(&response.status) && !response.body.trim().is_empty() {
        return Ok(Some(response.body));
    }
    Ok(None)
}

// ---- version 差分入力の読み取り ----

/// nix eval JSON ファイル（`{ "name": { "version", "repo", "changelog", "homepage" }, ... }`）を読む。
///
/// path が `None` / ファイル不存在なら空マップへ縮退する（差分取得不能は version+notes_url 縮退の契約）。
pub(crate) fn read_nix_versions(
    path: Option<&std::path::Path>,
) -> Result<std::collections::BTreeMap<String, NixPackage>> {
    let Some(path) = path else {
        return Ok(std::collections::BTreeMap::new());
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(std::collections::BTreeMap::new());
        }
        Err(error) => return Err(error.into()),
    };
    Ok(serde_json::from_str(&text)?)
}

// ---- ノート取得 ----

/// 許可ホスト https URL から本文を取得し、`RawReleaseNotes` へ翻訳する（raw 再取得可なので refetch_url=url）。
///
/// 許可外 URL・取得失敗・空本文は `None`（呼び出し側が version-only へ縮退する）。
pub(crate) fn fetch_from_source(url: &str) -> Result<Option<RawReleaseNotes>> {
    if !is_allowed_url(url) {
        return Ok(None);
    }
    Ok(safe_https_fetch(url)?.map(|text| RawReleaseNotes {
        text,
        notes_url: url.to_string(),
        refetch_url: Some(url.to_string()),
    }))
}

/// delta の出所に応じて生リリースノートを機械取得する（nix=Releases API/changelog、brew=常に空）。
pub(crate) fn fetch_release_notes(delta: &VersionDelta) -> Result<Option<RawReleaseNotes>> {
    match delta.source {
        DeltaSource::NixEval => fetch_nix_notes(
            delta.repo.as_deref(),
            delta.notes_source.as_deref(),
            delta.old.as_deref(),
            delta.new.as_deref(),
        ),
        // brew は機械取得しない（探索ヒント経由 AI 探索）。
        DeltaSource::BrewTap => Ok(None),
    }
}

/// brew cask `.rb` 定義を取得し、`homepage`（無ければ `url`）を探索ヒント URL として 1 件取り出す。
pub(crate) fn brew_notes_hint(brew_notes_base: Option<&str>, name: &str) -> Result<Option<String>> {
    let Some(base) = brew_notes_base else {
        return Ok(None);
    };
    let url = resolve_cask_url(base, name);
    if !is_allowed_url(&url) {
        return Ok(None);
    }
    Ok(safe_https_fetch(&url)?.as_deref().and_then(parse_cask_hint))
}

fn fetch_nix_notes(
    repo: Option<&str>,
    notes_source: Option<&str>,
    old: Option<&str>,
    new: Option<&str>,
) -> Result<Option<RawReleaseNotes>> {
    // 一次経路（Releases API）で本文が取れればそれを返し、空振りなら notes_source へフォールバックする。
    if let Some(repo) = repo.map(str::trim).filter(|s| !s.is_empty())
        && let Some((owner, repo_name)) = split_owner_repo(repo)
        && let Some(notes) = fetch_releases_range(owner, repo_name, old, new)?
    {
        return Ok(Some(notes));
    }
    if let Some(raw) = notes_source.map(str::trim).filter(|s| !s.is_empty())
        && let Some(plan) = resolve_nix_notes_source(raw)
        && let Some(notes) = fetch_plan(plan)?
    {
        return Ok(Some(notes));
    }
    Ok(None)
}

/// nix eval 由来 `notes_source`（信頼境界外 URL）の取得方式を表す純粋な解決結果。
enum NotesFetchPlan {
    Raw(String),
    ReleasesApi { api_url: String, notes_url: String },
}

fn fetch_plan(plan: NotesFetchPlan) -> Result<Option<RawReleaseNotes>> {
    match plan {
        NotesFetchPlan::Raw(url) => fetch_from_source(&url),
        NotesFetchPlan::ReleasesApi { api_url, notes_url } => {
            fetch_release_api(&api_url, &notes_url)
        }
    }
}

fn fetch_release_api(api_url: &str, notes_url: &str) -> Result<Option<RawReleaseNotes>> {
    if !is_allowed_url(api_url) {
        return Ok(None);
    }
    let json = match http_get(api_url, &[GITHUB_ACCEPT_HEADER])? {
        Some(response)
            if (200..300).contains(&response.status) && !response.body.trim().is_empty() =>
        {
            response.body
        }
        // 接続失敗・非 2xx・空本文は取得不能（空）。
        _ => return Ok(None),
    };
    Ok(extract_release_body(&json).map(|body| RawReleaseNotes {
        text: body,
        notes_url: notes_url.to_string(),
        refetch_url: None,
    }))
}

/// GitHub Releases API で `owner/repo` の `(old, new]` 範囲のリリースノートを取得して連結する。
fn fetch_releases_range(
    owner: &str,
    repo: &str,
    old: Option<&str>,
    new: Option<&str>,
) -> Result<Option<RawReleaseNotes>> {
    let mut bodies: Vec<(String, String)> = Vec::new();
    for page in 1..=MAX_RELEASE_PAGES {
        let api_url = releases_list_url(owner, repo, page);
        if !is_allowed_url(&api_url) {
            return Ok(None);
        }
        let json = match fetch_releases_page(&api_url, owner, repo)? {
            Some(text) => text,
            None => return Ok(None),
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
        refetch_url: None,
    }))
}

fn join_release_bodies(mut bodies: Vec<(String, String)>) -> String {
    bodies.sort_by(|a, b| version_ordering(&a.0, &b.0));
    bodies
        .into_iter()
        .map(|(_, body)| body)
        .collect::<Vec<_>>()
        .join(RELEASE_BODY_SEPARATOR)
}

/// GitHub Releases API 取得に添える Accept ヘッダ。
const GITHUB_ACCEPT_HEADER: Header<'static> = ("Accept", "application/vnd.github+json");
/// GitHub API バージョン固定ヘッダ。
const GITHUB_API_VERSION_HEADER: Header<'static> = ("X-GitHub-Api-Version", "2022-11-28");

/// nix eval 由来 `notes_source` URL を「生ノートが返る取得先」へ翻訳する純粋関数。
fn resolve_nix_notes_source(url: &str) -> Option<NotesFetchPlan> {
    let rest = url.strip_prefix("https://github.com/")?;
    let mut segments = rest.splitn(3, '/');
    let owner = non_empty(segments.next())?;
    let repo = non_empty(segments.next())?;
    let tail = segments.next().unwrap_or("");
    if let Some(blob_tail) = tail.strip_prefix("blob/") {
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
    None
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn extract_release_body(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let body = value.get("body")?.as_str()?;
    if body.trim().is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty())
}

fn split_owner_repo(repo: &str) -> Option<(&str, &str)> {
    let (owner, name) = repo.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some((owner, name))
}

fn releases_list_url(owner: &str, repo: &str, page: u32) -> String {
    format!(
        "https://api.github.com/repos/{owner}/{repo}/releases?per_page={RELEASES_PER_PAGE}&page={page}"
    )
}

fn releases_page_url(owner: &str, repo: &str) -> String {
    format!("https://github.com/{owner}/{repo}/releases")
}

/// GitHub Releases API の 1 ページを reqwest で取得する。
///
/// `GITHUB_TOKEN` は [`http_get`] が GitHub 系ホストにだけ `Authorization: Bearer` で添える（reqwest の
/// ヘッダ値として渡すため process argv には一切現れない。curl `--config -` 相当の秘匿を in-process で達成）。
/// レート/一過性失敗（403 secondary・429・5xx・接続失敗）は [`http_get`] が有界バックオフで再試行する。
/// 2xx 非空本文だけを `Some` で返し、接続失敗・非 2xx・空本文は `None`（取得不能）。
fn fetch_releases_page(api_url: &str, owner: &str, repo: &str) -> Result<Option<String>> {
    let headers: [Header<'_>; 2] = [GITHUB_ACCEPT_HEADER, GITHUB_API_VERSION_HEADER];
    let Some(response) = http_get(api_url, &headers)? else {
        return Ok(None);
    };
    match response.status {
        200 if !response.body.trim().is_empty() => Ok(Some(response.body)),
        status => {
            if status == 401 || status == 403 || status == 429 {
                eprintln!(
                    "update-history notes: releases API failure: HTTP {status} for {owner}/{repo}"
                );
            }
            Ok(None)
        }
    }
}

fn parse_releases(json: &str) -> Option<Vec<Release>> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let array = value.as_array()?;
    Some(
        array
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
            .collect(),
    )
}

struct Release {
    tag_name: String,
    name: String,
    body: String,
}

impl Release {
    fn in_range_version(&self, old: Option<&str>, new: Option<&str>) -> Option<String> {
        if self.body.trim().is_empty() {
            return None;
        }
        let version = release_version(&self.tag_name, &self.name)?;
        version_in_range(&version, old, new).then_some(version)
    }
}

/// `notes_base` と package 名から cask 取得 URL（`Casks/<subdir>/<name>.rb`）を構築する純粋関数。
fn resolve_cask_url(base: &str, name: &str) -> String {
    if is_cask_base(base) {
        let subdir = cask_subdir(name);
        format!("{base}{subdir}/{name}.rb")
    } else {
        format!("{base}{name}")
    }
}

fn parse_cask_hint(rb: &str) -> Option<String> {
    extract_dsl_string(rb, "homepage").or_else(|| extract_dsl_string(rb, "url"))
}

fn extract_dsl_string(rb: &str, key: &str) -> Option<String> {
    for line in rb.lines() {
        let trimmed = line.trim_start();
        let Some(after_key) = trimmed.strip_prefix(key) else {
            continue;
        };
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

fn is_cask_base(base: &str) -> bool {
    base.ends_with("/Casks/") || base == "Casks/"
}

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
    //! nix eval 読み取り・URL 変換・cask URL 構築・cask ヒント抽出・GitHub ヘッダ定数と、安全 fetch client
    //! 構成（redirect 不追従・https 限定・有界）・本文上限読みを network 抜きで固定する。

    use super::*;

    #[test]
    fn http_client_builds_and_read_capped_truncates_at_limit() -> Result<()> {
        // 構築が成功し（redirect/https/timeout が矛盾しない）、共有 client を返せる。
        let _ = http_client()?;
        // 上限を超える本文は limit バイトで打ち切って読む（巨大本文を全読みしない）。
        let body = vec![b'x'; 100];
        assert_eq!(read_capped(body.as_slice(), 10).len(), 10);
        assert_eq!(
            read_capped([b'a', b'b', b'c'].as_slice(), MAX_RESPONSE_BYTES),
            "abc"
        );
        Ok(())
    }

    #[test]
    fn http_user_agent_is_fixed_nonempty_crate_ua() {
        // GitHub API は UA 無しを 403 で拒否するため、共有 client へ付与する固定 UA は非空で crate 名を含む。
        assert!(HTTP_USER_AGENT.starts_with("dotfiles-update-history/"));
        assert!(HTTP_USER_AGENT.len() > "dotfiles-update-history/".len());
    }

    #[test]
    fn github_headers_are_fixed() {
        // Releases API へ添える Accept / API バージョンヘッダが固定であることを確認する。
        assert_eq!(
            GITHUB_ACCEPT_HEADER,
            ("Accept", "application/vnd.github+json")
        );
        assert_eq!(
            GITHUB_API_VERSION_HEADER,
            ("X-GitHub-Api-Version", "2022-11-28")
        );
    }

    #[test]
    fn read_nix_versions_parses_and_degrades() -> Result<()> {
        let mut path = std::env::temp_dir();
        path.push(format!("dotfiles-uh-nix-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"neovim":{"version":"0.11.0","repo":"neovim/neovim","changelog":"https://github.com/neovim/neovim/blob/master/CHANGELOG.md"},"git":{"version":"2.54.0"}}"#,
        )?;
        let map = read_nix_versions(Some(&path))?;
        assert_eq!(
            map.get("neovim").map(|p| p.version.as_str()),
            Some("0.11.0")
        );
        assert_eq!(
            map.get("neovim").map(|p| p.repo.as_str()),
            Some("neovim/neovim")
        );
        assert_eq!(map.get("git").map(|p| p.repo.as_str()), Some(""));
        let _ = std::fs::remove_file(&path);
        // None / 不存在は空マップへ縮退。
        assert!(read_nix_versions(None)?.is_empty());
        let mut missing = std::env::temp_dir();
        missing.push("dotfiles-uh-nix-missing.json");
        let _ = std::fs::remove_file(&missing);
        assert!(read_nix_versions(Some(&missing))?.is_empty());
        Ok(())
    }

    #[test]
    fn legacy_notes_source_key_via_alias() -> Result<()> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "dotfiles-uh-nix-legacy-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"{"git":{"version":"2.54.0","notes_source":"https://github.com/git/git"}}"#,
        )?;
        let map = read_nix_versions(Some(&path))?;
        assert_eq!(
            map.get("git").map(|p| p.notes_source.as_str()),
            Some("https://github.com/git/git")
        );
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn blob_url_resolves_to_raw_and_releases_tag_to_api() {
        match resolve_nix_notes_source("https://github.com/o/r/blob/v1.2.3/CHANGELOG.md") {
            Some(NotesFetchPlan::Raw(url)) => assert_eq!(
                url,
                "https://raw.githubusercontent.com/o/r/v1.2.3/CHANGELOG.md"
            ),
            _ => panic!("expected Raw"),
        }
        match resolve_nix_notes_source("https://github.com/o/r/releases/tag/v2.0.0") {
            Some(NotesFetchPlan::ReleasesApi { api_url, notes_url }) => {
                assert_eq!(
                    api_url,
                    "https://api.github.com/repos/o/r/releases/tags/v2.0.0"
                );
                assert_eq!(notes_url, "https://github.com/o/r/releases/tag/v2.0.0");
            }
            _ => panic!("expected ReleasesApi"),
        }
        // repo root / 非 github は None。
        assert!(resolve_nix_notes_source("https://github.com/o/r").is_none());
        assert!(resolve_nix_notes_source("https://gitlab.com/o/r/blob/v1/CHANGELOG").is_none());
    }

    #[test]
    fn extract_release_body_reads_body_field() {
        assert_eq!(
            extract_release_body(r#"{"body":"notes"}"#).as_deref(),
            Some("notes")
        );
        assert_eq!(extract_release_body(r#"{"body":"  "}"#), None);
        assert_eq!(extract_release_body(r#"{}"#), None);
    }

    #[test]
    fn cask_url_resolves_letter_and_font_subdir() {
        let base = "https://raw.githubusercontent.com/homebrew/homebrew-cask/deadbeef/Casks/";
        assert_eq!(
            resolve_cask_url(base, "firefox"),
            format!("{base}f/firefox.rb")
        );
        assert_eq!(
            resolve_cask_url(base, "Discord"),
            format!("{base}d/Discord.rb")
        );
        assert_eq!(
            resolve_cask_url(base, "font-cica"),
            format!("{base}font/font-cica.rb")
        );
    }

    #[test]
    fn parse_cask_hint_prefers_homepage_then_url() {
        let rb = "cask \"firefox\" do\n  url \"https://download.example/firefox.dmg\"\n  homepage \"https://www.mozilla.org/firefox/\"\nend\n";
        assert_eq!(
            parse_cask_hint(rb).as_deref(),
            Some("https://www.mozilla.org/firefox/")
        );
        let rb_no_home =
            "cask \"x\" do\n  url \"https://github.com/o/r/releases/download/v1/x.zip\"\nend\n";
        assert_eq!(
            parse_cask_hint(rb_no_home).as_deref(),
            Some("https://github.com/o/r/releases/download/v1/x.zip")
        );
        assert!(
            parse_cask_hint("cask \"x\" do\n  url_template \"https://example/#{version}\"\nend\n")
                .is_none()
        );
    }

    #[test]
    fn fetch_nix_notes_degrades_without_network_when_no_hints() -> Result<()> {
        // repo/notes_source ともに無し → curl を踏まず空（hermetic）。
        assert!(fetch_nix_notes(None, None, None, None)?.is_none());
        // 非 github の notes_source は機械取得 plan（github 専用の resolve_nix_notes_source）を導かず変換段で
        // 空（curl を踏まない）。機械取得は github の構造化エンドポイントに閉じる（AI fetch のみ github 外へ広がる）。
        assert!(
            fetch_nix_notes(None, Some("https://example.com/changelog"), None, None)?.is_none()
        );
        Ok(())
    }

    #[test]
    fn brew_hint_without_base_is_none() -> Result<()> {
        assert!(brew_notes_hint(None, "firefox")?.is_none());
        Ok(())
    }

    #[test]
    fn is_github_host_matches_github_family_only() {
        // token 添付・rate limit 再試行の対象になる GitHub 系 host（正規化済み小文字）。
        assert!(is_github_host("github.com"));
        assert!(is_github_host("api.github.com"));
        assert!(is_github_host("raw.githubusercontent.com"));
        // `github.com` の厳密なサブドメインも GitHub 系。
        assert!(is_github_host("codeload.github.com"));
        // 機械取得で叩く 3 ホスト以外（`objects.githubusercontent.com` 等の release asset 配信は
        // raw.githubusercontent.com の子孫でないため）には token を添えない（保守的に最小スコープ）。
        assert!(!is_github_host("objects.githubusercontent.com"));
        // GitHub 以外（cargo / iterm2 等のノート所在）には token を添えない。
        assert!(!is_github_host("doc.rust-lang.org"));
        assert!(!is_github_host("iterm2.com"));
        assert!(!is_github_host("example.com"));
        assert!(!is_github_host("gitlab.com"));
        // 接尾辞偽装（`notgithub.com`・`github.com.evil.com`）は GitHub 系とみなさない。
        assert!(!is_github_host("notgithub.com"));
        assert!(!is_github_host("github.com.evil.com"));
        assert!(!is_github_host("evilgithub.com"));
    }

    /// テスト用に Connected な Attempt を組む。
    fn connected(
        status: u16,
        body: &str,
        retry_after: Option<&str>,
        remaining: Option<&str>,
    ) -> Attempt {
        Attempt::Connected {
            status,
            body: body.to_string(),
            retry_after: retry_after.map(str::to_string),
            rate_remaining: remaining.map(str::to_string),
        }
    }

    fn is_retry(attempt: &Attempt, github_host: bool) -> bool {
        matches!(retry_decision(attempt, github_host), RetryDecision::Retry)
    }

    #[test]
    fn retry_decision_retries_transient_and_finalizes_terminal() {
        // 接続失敗・429・5xx は GitHub でも非 GitHub でも再試行対象。
        assert!(is_retry(&Attempt::SendError, true));
        assert!(is_retry(&Attempt::SendError, false));
        assert!(is_retry(&connected(429, "", None, None), true));
        assert!(is_retry(&connected(429, "", None, None), false));
        assert!(is_retry(&connected(500, "", None, None), true));
        assert!(is_retry(&connected(503, "", None, None), false));
        // 200/404 は即確定（再試行しない）。
        assert!(!is_retry(&connected(200, "ok", None, None), true));
        assert!(!is_retry(&connected(404, "", None, None), true));
    }

    #[test]
    fn retry_decision_handles_403_rate_limit_only_for_github() {
        // GitHub の 403 secondary rate limit（本文兆候あり）は再試行対象。
        assert!(is_retry(
            &connected(403, "You have exceeded a secondary rate limit", None, None),
            true
        ));
        assert!(is_retry(
            &connected(403, "API rate limit exceeded for user", None, None),
            true
        ));
        // primary rate limit 枯渇（remaining=0）の 403 は reset 待ちが長すぎるため再試行せず None へ縮退する。
        match retry_decision(
            &connected(403, "API rate limit exceeded", None, Some("0")),
            true,
        ) {
            RetryDecision::Done(None) => {}
            _ => panic!("primary rate limit は再試行せず縮退"),
        }
        // rate limit 兆候の無い 403（純粋な権限拒否）は即確定（縮退）。
        match retry_decision(&connected(403, "forbidden", None, None), true) {
            RetryDecision::Done(Some(response)) => assert_eq!(response.status, 403),
            _ => panic!("rate limit 兆候の無い 403 は確定"),
        }
        // GitHub 以外のホストでは 403 を rate limit とみなさず即確定（rate limit 概念は GitHub 固有）。
        assert!(!is_retry(
            &connected(403, "secondary rate limit", None, None),
            false
        ));
    }

    #[test]
    fn backoff_wait_secs_uses_exponential_then_retry_after_capped() {
        // Retry-After 無し → 指数（1, 2, 4 ...）を上限で頭打ち。
        assert_eq!(
            backoff_wait_secs(0, None, 0, BACKOFF_TOTAL_CAP_SECS),
            Some(1)
        );
        assert_eq!(
            backoff_wait_secs(1, None, 0, BACKOFF_TOTAL_CAP_SECS),
            Some(2)
        );
        assert_eq!(
            backoff_wait_secs(2, None, 0, BACKOFF_TOTAL_CAP_SECS),
            Some(4)
        );
        // 指数項が BACKOFF_MAX_SECS を超えても頭打ち。
        assert_eq!(
            backoff_wait_secs(20, None, 0, BACKOFF_TOTAL_CAP_SECS),
            Some(BACKOFF_MAX_SECS)
        );
        // Retry-After（秒）優先。上限でクランプ。
        assert_eq!(
            backoff_wait_secs(0, Some("5"), 0, BACKOFF_TOTAL_CAP_SECS),
            Some(5)
        );
        assert_eq!(
            backoff_wait_secs(0, Some("9999"), 0, BACKOFF_TOTAL_CAP_SECS),
            Some(BACKOFF_MAX_SECS)
        );
        // HTTP-date 形式の Retry-After は秒数化できず指数へフォールバック。
        assert_eq!(
            backoff_wait_secs(
                0,
                Some("Wed, 21 Oct 2026 07:28:00 GMT"),
                0,
                BACKOFF_TOTAL_CAP_SECS
            ),
            Some(1)
        );
        // 総待機上限を超える待機は None（諦める）。
        assert_eq!(backoff_wait_secs(0, Some("10"), 55, 60), None);
        assert_eq!(backoff_wait_secs(0, Some("5"), 55, 60), Some(5));
    }

    #[test]
    fn parse_retry_after_secs_accepts_delta_seconds_only() {
        assert_eq!(parse_retry_after_secs("0"), Some(0));
        assert_eq!(parse_retry_after_secs("  42 "), Some(42));
        assert_eq!(
            parse_retry_after_secs("Wed, 21 Oct 2026 07:28:00 GMT"),
            None
        );
        assert_eq!(parse_retry_after_secs(""), None);
        assert_eq!(parse_retry_after_secs("-1"), None);
    }

    #[test]
    fn primary_and_secondary_rate_limit_signals() {
        assert!(is_primary_rate_limited(Some("0")));
        assert!(is_primary_rate_limited(Some(" 0 ")));
        assert!(!is_primary_rate_limited(Some("1")));
        assert!(!is_primary_rate_limited(None));
        assert!(is_secondary_rate_limited(
            "You have exceeded a SECONDARY RATE LIMIT"
        ));
        assert!(is_secondary_rate_limited(
            "triggered an abuse detection mechanism"
        ));
        assert!(is_secondary_rate_limited("API rate limit exceeded"));
        assert!(!is_secondary_rate_limited("forbidden: bad credentials"));
    }
}

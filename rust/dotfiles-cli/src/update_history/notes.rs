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
//! - **nix eval 由来**: delta が運ぶ `repo`（GitHub Releases API で `(old, new]` 範囲を取得）を一次に、成功応答で
//!   該当本文が無い時だけ `notes_source`（changelog blob→raw / releases/tag→Releases API `.body`）へフォールバックする。
//!   transport・HTTP status・応答構文の失敗は「ノート無し」へ再分類せず伝播する。
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
use anyhow::Context;

/// 接続確立の上限。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// リクエスト全体（接続〜本文読み取り）の上限。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// 1 レスポンス本文の読み取り上限（バイト）。超過分は読まずに打ち切る。
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

/// 共有 blocking client（redirect 不追従・https 限定・有界 timeout）。初回アクセスで 1 度だけ構築する。
static HTTP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

/// 全 HTTP 取得に添える固定 User-Agent。
///
/// GitHub REST API は有効な `User-Agent` がない request を reject するため、共有 client の default header として
/// 1 箇所で付与し、GitHub API・raw.githubusercontent.com 双方の経路に効かせる。
/// Evidence: <https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api?apiVersion=2022-11-28#user-agent-required>
const HTTP_USER_AGENT: &str = concat!("dotfiles-update-history/", env!("CARGO_PKG_VERSION"));

/// 1 リクエストに添える追加ヘッダ（名前と値の組）。
type Header<'a> = (&'a str, &'a str);

/// HTTP GET の最小レスポンス（status と上限付き本文）。
struct HttpResponse {
    status: reqwest::StatusCode,
    body: String,
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
/// GitHub 系ホスト（[`is_github_host`]）にだけ `GITHUB_TOKEN` を `Authorization: Bearer` で添える。token は
/// reqwest のヘッダ値として渡すため argv/URL/ログに出ず、GitHub 以外のホストには**絶対に付けない**（漏えい防止）。
/// transport error と HTTP status の再試行可否は、この generic fetcher で推測しない。transport error は伝播する。
///
/// HTTP response は status と本文をそのまま返す。本文は [`MAX_RESPONSE_BYTES`] までで打ち切って読む。
fn http_get(url: &str, headers: &[Header<'_>]) -> Result<HttpResponse> {
    let github_host = host_of(url).is_some_and(|host| is_github_host(&host));
    // GitHub 系ホストにだけ token を添える（host 一致時のみ。token 漏えい防止）。
    let authorization = if github_host {
        github_token().map(|token| format!("Bearer {token}"))
    } else {
        None
    };
    http_get_once(url, headers, authorization.as_deref())
}

/// 1 回だけ GET を試み、status と有界本文を返す。
///
/// `reqwest::blocking::RequestBuilder::send` が返す transport error は再試行可否や「ノート無し」へ分類せず伝播する。
/// `Response::status` は HTTP status を返すだけなので、その意味付けは endpoint 固有の呼出元だけが行う。
/// - <https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.RequestBuilder.html#method.send>
/// - <https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.Response.html#method.status>
fn http_get_once(
    url: &str,
    headers: &[Header<'_>],
    authorization: Option<&str>,
) -> Result<HttpResponse> {
    let client = http_client()?;
    let base = headers
        .iter()
        .fold(client.get(url), |request, (name, value)| {
            request.header(*name, *value)
        });
    let request = match authorization {
        Some(authorization) => base.header("Authorization", authorization),
        None => base,
    };
    // reqwest の transport error は原因ごとの再試行可否をこの層で判定しない。
    // HTTP response を受け取れない失敗は、状態へ変換せず呼び出し元へ伝播する。
    let response = request.send()?;
    let status = response.status();
    let body = read_capped(response, MAX_RESPONSE_BYTES)?;
    Ok(HttpResponse { status, body })
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

/// 任意の [`Read`] を `limit` バイトまで読み、UTF-8 として lossy にデコードする純粋規約。
///
/// `take` で読み取り段階から上限を掛け、巨大本文を全量バッファしない（資源枯渇防止）。読み取り失敗は
/// 部分本文へ縮退せず伝播する。`read_to_end` の宛先 Vec はこの関数内に閉じた I/O バッファである。
fn read_capped<R: Read>(reader: R, limit: u64) -> Result<String> {
    let mut buffer = Vec::new();
    reader.take(limit).read_to_end(&mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

/// GitHub Releases API のページサイズ（API 上限は 100）。
const RELEASES_PER_PAGE: u32 = 100;
/// Releases API のページング取得上限ページ数。
const MAX_RELEASE_PAGES: u32 = 3;
/// 範囲取得した複数リリース `.body` を連結する区切り（新しい順に積む）。
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
/// host allowlist 検査は呼び出し側の責務（この primitive は host を再判定しない）。空本文だけを `None` とする。
/// reqwest が返した非成功 status は、資源不在・一時障害・空取得などへ分類せず失敗として伝播する。
///
/// Evidence: `http::StatusCode::is_success` は 200..=299 の判定を定義するだけである。GitHub の 403/429
/// rate-limit response は `Retry-After`、`x-ratelimit-remaining`、`x-ratelimit-reset` に従う必要があり、本文の
/// 部分一致や固定 backoff で再試行しない。
/// - <https://docs.rs/http/1.4.1/http/status/struct.StatusCode.html#method.is_success>
/// - <https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api?apiVersion=2022-11-28#rate-limit-errors>
pub(crate) fn safe_https_fetch(url: &str) -> Result<Option<String>> {
    let response = http_get(url, &[])?;
    require_success_status(url, response.status)?;
    Ok((!response.body.trim().is_empty()).then_some(response.body))
}

/// reqwest が公開する `StatusCode::is_success`（2xx）の意味だけを用い、非成功 status をリソース不在などの
/// domain 状態へ翻訳しない。
fn require_success_status(url: &str, status: reqwest::StatusCode) -> Result<()> {
    if !status.is_success() {
        anyhow::bail!("HTTPS GET `{url}` returned non-success HTTP status {status}");
    }
    Ok(())
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
/// 許可外 URL と成功した空本文だけを `None` とする。transport error と non-2xx は外部 failure として伝播する。
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
        return Ok(package_brew_hint(name));
    };
    let url = resolve_cask_url(base, name);
    if !is_allowed_url(&url) {
        return Ok(package_brew_hint(name));
    }
    Ok(safe_https_fetch(&url)?
        .as_deref()
        .and_then(parse_cask_hint)
        .or_else(|| package_brew_hint(name)))
}

fn package_brew_hint(name: &str) -> Option<String> {
    match name {
        "bitwarden" => Some("https://bitwarden.com/help/releasenotes/".to_string()),
        "codex-app" => Some("https://developers.openai.com/codex/changelog".to_string()),
        _ => None,
    }
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
    let response = http_get(api_url, &[GITHUB_ACCEPT_HEADER])?;
    if response.status != reqwest::StatusCode::OK {
        anyhow::bail!(
            "GitHub release API GET `{api_url}` returned HTTP status {}",
            response.status
        );
    }
    if response.body.trim().is_empty() {
        return Ok(None);
    }
    let json = response.body;
    Ok(extract_release_body(&json)?.map(|body| RawReleaseNotes {
        text: body,
        notes_url: notes_url.to_string(),
        refetch_url: None,
    }))
}

/// GitHub Releases API で `owner/repo` のリリースノートを取得して連結する。
///
/// 一次は厳密な `(old, new]` 範囲（[`collect_release_bodies`]）。API 取得自体は成功したが範囲に該当する本文が
/// 一件も無い場合（タグ表記揺れ・old 排他境界・新版リリース未タグ等で窓が空になる）、repo-backed パッケージが
/// 機械 seed を全く持てず route=none に落ちるのを防ぐため、`old` より後のリリース本文だけを seed にする
/// 緩和フォールバックを一度だけ試みる（構造化 Releases API の `.body` のみで landing page HTML は混ぜない）。
/// API の transport / status / response-shape が失敗した場合はフォールバックで隠さず error を伝播する。
fn fetch_releases_range(
    owner: &str,
    repo: &str,
    old: Option<&str>,
    new: Option<&str>,
) -> Result<Option<RawReleaseNotes>> {
    fetch_releases_range_with(owner, repo, old, new, &|api_url| {
        fetch_releases_page(api_url, owner, repo)
    })
}

/// [`fetch_releases_range`] 本体。Releases API ページ取得を `fetch_page`（api_url→生 JSON）へ注入し、network 抜きで
/// 緩和制御を決定論に固定できるようにする。本番は [`fetch_releases_page`] を渡す。
fn fetch_releases_range_with(
    owner: &str,
    repo: &str,
    old: Option<&str>,
    new: Option<&str>,
    fetch_page: &dyn Fn(&str) -> Result<String>,
) -> Result<Option<RawReleaseNotes>> {
    // 厳密 `(old, new]` 範囲を集める。空=取得成功だが窓が空（緩和の対象）、非空=本文在り（そのまま seed）。
    // transport / HTTP / JSON error は `collect_release_bodies` から伝播し、緩和 fetch を始めない。
    let strict = collect_release_bodies(owner, repo, old, new, 1, Vec::new(), fetch_page)?;
    if !strict.is_empty() {
        return Ok(Some(release_notes_from(owner, repo, strict)));
    }
    // 厳密窓が空（取得は成功）かつ old 境界が在るときのみ、new 側の上限を外した `(old, ..)` の最新リリースを seed
    // に緩和する。old 境界以前の本文を seed に混ぜると、既に適用済みの古い release notes を今回更新として LLM が
    // 抽出する false positive になるため、緩和しても old より後という下限は保つ。
    // タグ表記揺れ・old 排他境界・新版リリース未タグ等で `(old, new]` 窓が空になっても、repo-backed パッケージが
    // 機械 seed を全く持てず route=none に落ちるのを防ぐ。old が元から無い（初回固定）場合は既に最広の範囲であり、
    // 緩和しても結果は変わらないため試みない（構造化 Releases API の `.body` のみ。landing page HTML は混ぜない）。
    if old.is_none() {
        return Ok(None);
    }
    let relaxed = collect_release_bodies(owner, repo, old, None, 1, Vec::new(), fetch_page)?;
    if relaxed.is_empty() {
        return Ok(None);
    }
    Ok(Some(release_notes_from(owner, repo, relaxed)))
}

/// 集めた `(version, body)` 列を seed [`RawReleaseNotes`] へ畳む（連結は新しい版が先頭）。
fn release_notes_from(owner: &str, repo: &str, bodies: Vec<(String, String)>) -> RawReleaseNotes {
    RawReleaseNotes {
        text: join_release_bodies(bodies),
        notes_url: releases_page_url(owner, repo),
        refetch_url: None,
    }
}

/// `(old, new]` 範囲の `(version, body)` をページ走査で集める（不変 accumulator の再帰）。
///
/// `page` を 1 から `MAX_RELEASE_PAGES` まで進め、各ページの in-range release を `acc` に不変連結して次ページへ
/// 渡す。許可外 URL・ページ取得失敗・JSON 解析失敗はいずれも error として伝播する。短いページ
/// （`< RELEASES_PER_PAGE`）に達するか最終ページまで進めば、集めた列を返す。
fn collect_release_bodies(
    owner: &str,
    repo: &str,
    old: Option<&str>,
    new: Option<&str>,
    page: u32,
    acc: Vec<(String, String)>,
    fetch_page: &dyn Fn(&str) -> Result<String>,
) -> Result<Vec<(String, String)>> {
    if page > MAX_RELEASE_PAGES {
        return Ok(acc);
    }
    let api_url = releases_list_url(owner, repo, page);
    if !is_allowed_url(&api_url) {
        anyhow::bail!("refusing structurally disallowed GitHub releases URL `{api_url}`");
    }
    let json = fetch_page(&api_url)?;
    let releases = parse_releases(&json)?;
    let page_len = releases.len();
    let extended: Vec<(String, String)> = acc
        .into_iter()
        .chain(releases.into_iter().filter_map(|release| {
            release
                .in_range_version(old, new)
                .map(|version| (version, release.body))
        }))
        .collect();
    if (page_len as u32) < RELEASES_PER_PAGE {
        return Ok(extended);
    }
    collect_release_bodies(owner, repo, old, new, page + 1, extended, fetch_page)
}

/// 範囲取得した `(version, body)` を version の semver 降順（新しい順）に連結する。
///
/// 並べ替えキーは [`VersionKey`]（[`version_ordering`] へ委譲する `Ord` ラッパ）で `(version, index)` を作り、
/// `BTreeMap` の安定順序を得てから `.rev()` で降順に反転する（可変ソートを使わない）。`index` は同一 version の
/// 複数 body を入力出現順に保つためのタイブレーク。新しい版を先頭へ積むのは、下流（[`super::llm`]）が seed を
/// 先頭から `MAX_NOTES_CHARS` で切り詰めるとき、肝心の最新版差分が末尾切り捨てで落ちないようにするためである。
fn join_release_bodies(bodies: Vec<(String, String)>) -> String {
    bodies
        .into_iter()
        .enumerate()
        .map(|(index, (version, body))| ((VersionKey(version), index), body))
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_values()
        .rev()
        .collect::<Vec<_>>()
        .join(RELEASE_BODY_SEPARATOR)
}

/// version 文字列を [`version_ordering`]（semver 規則）で順序づける `Ord` ラッパ（`BTreeMap` キー用）。
#[derive(PartialEq, Eq)]
struct VersionKey(String);

impl Ord for VersionKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // semver 上 Equal の異表記でも `BTreeMap` キーの一貫性のため生文字列でタイブレークする。
        version_ordering(&self.0, &other.0).then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for VersionKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// GitHub Releases API 取得に添える Accept ヘッダ。
const GITHUB_ACCEPT_HEADER: Header<'static> = ("Accept", "application/vnd.github+json");
/// GitHub API バージョン固定ヘッダ。
const GITHUB_API_VERSION_HEADER: Header<'static> = ("X-GitHub-Api-Version", "2022-11-28");

/// nix eval 由来 `notes_source` URL を「生ノートが返る取得先」へ翻訳する純粋関数。
///
/// `github.com` の blob（→raw）/ releases/tag（→Releases API）は構造化エンドポイントへ変換する。それ以外でも
/// 許可host（[`is_allowed_url`] が真＝[`host_of`] の構造的検査を通る公開 https）の URL は、生 changelog
/// （`https://raw.githubusercontent.com/.../CHANGELOG.md`・`https://gitlab.com/...` 等）を `Raw` で fallback
/// 取得する。SSRF は `is_allowed_url`（host_of の IP/localhost/単一ラベル/credential 拒否）で唯一ゲートされる。
///
/// ただし landing page（サイト/リポジトリのルート。`https://github.com/owner/repo`・`https://www.docker.com/` 等）は
/// 機械 seed に固定しない。これらの本文は HTML の `<head>`/ナビゲーション chrome であり、リリースノート本文を
/// 含まない（先頭を [`super::llm`] が切り詰めると chrome だけが残り抽出 0 件に倒れる）。landing page は seed を
/// `None` にして AI の `fetch_url` 探索へ回す（agent は `/releases` 等を自分で組み立てて実ノート本文を読む）。
fn resolve_nix_notes_source(url: &str) -> Option<NotesFetchPlan> {
    if url == "https://developer.chrome.com/docs/chromedriver/downloads" {
        return None;
    }
    // github.com の URL は構造化変換（blob→raw / releases-tag→API）に当たるものだけ機械 seed にする。
    // それ以外の github.com URL（bare repo root `/owner/repo`・issues・wiki 等）は landing page（HTML chrome）で
    // あり生本文を Raw 取得すると先頭が chrome だけになり抽出 0 件に倒れるため、機械 seed にせず None を返す
    // （seed 無しで AI の fetch_url 探索へ回す）。
    if is_github_landing_host(url) {
        return resolve_github_notes_source(url);
    }
    // github 以外の URL は、許可host かつ landing page（サイトルート）でなければ生本文を fallback 取得する。
    if is_allowed_url(url) && !is_landing_page_url(url) {
        return Some(NotesFetchPlan::Raw(url.to_string()));
    }
    None
}

/// URL が `github.com`（構造化変換を試みる host）かを判定する純粋関数。
fn is_github_landing_host(url: &str) -> bool {
    url.starts_with("https://github.com/")
}

/// URL が landing page（document path を持たないサイトルート）かを判定する純粋関数。
///
/// path が空・`/` のみのホストルート（`https://www.docker.com/`・`https://neovim.io` 等）は HTML chrome だけで
/// リリースノート本文を含まないため、機械 seed に使わない。document path（`/blog/...`・`/CHANGELOG.md` 等）を
/// 持つ URL は landing page ではない。github の bare repo root（`/owner/repo`）は [`resolve_nix_notes_source`]
/// が github host 分岐で先に弾くため、ここでは github 以外の host 直下の path 有無だけを見る。
fn is_landing_page_url(url: &str) -> bool {
    match url::Url::parse(url) {
        Ok(parsed) => parsed.path().trim_matches('/').is_empty(),
        Err(_) => false,
    }
}

/// `github.com` の blob/releases-tag URL を構造化取得先へ翻訳する純粋関数（該当しなければ `None`）。
///
/// bare repo root（`https://github.com/owner/repo`。blob/releases-tag いずれにも当たらない tail 空）は landing
/// page（HTML chrome）であり機械 seed に使わないため `None` を返す（呼び出し側で AI 探索へ回る）。
fn resolve_github_notes_source(url: &str) -> Option<NotesFetchPlan> {
    let rest = url.strip_prefix("https://github.com/")?;
    let (owner, after_owner) = rest.split_once('/')?;
    let owner = non_empty(Some(owner))?;
    // owner の次の segment が repo、それ以降（無ければ空）が tail。
    let (repo, tail) = match after_owner.split_once('/') {
        Some((repo, tail)) => (repo, tail),
        None => (after_owner, ""),
    };
    let repo = non_empty(Some(repo))?;
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
        // slash を含む tag（`foo/bar`）は API path segment を壊すため percent-encode してから補間する。
        let encoded_tag = encode_tag_segment(tag);
        return Some(NotesFetchPlan::ReleasesApi {
            api_url: format!(
                "https://api.github.com/repos/{owner}/{repo}/releases/tags/{encoded_tag}"
            ),
            notes_url: url.to_string(),
        });
    }
    None
}

/// release tag を Releases API の単一 path segment へ percent-encode する純粋関数（`/`→`%2F` 等）。
///
/// `releases/tags/{tag}` の `{tag}` は 1 path segment であり、`foo/bar` のような slash 入り tag を生で補間すると
/// API path 階層が崩れ 404 になる。`percent-encoding` crate の `NON_ALPHANUMERIC` を基底に、path segment で
/// 安全な `-._~` だけ除外する集合で encode する（手組み encode はしない）。
fn encode_tag_segment(tag: &str) -> String {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    // unreserved（RFC 3986）`-._~` は素のまま残し、`/` を含む他の非英数字は encode する。
    const TAG_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');
    utf8_percent_encode(tag, TAG_SEGMENT).to_string()
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// GitHub Releases API の単一 release 応答から非空の本文を取り出す。
///
/// JSON 構文・構造の不正は外部 API 応答の異常であり、本文が無い正常な release と同一視しない。前者は
/// 呼出元へ伝播し、後者だけを `Ok(None)` とする。
fn extract_release_body(json: &str) -> Result<Option<String>> {
    let value: serde_json::Value =
        serde_json::from_str(json).context("failed to parse GitHub release JSON")?;
    let object = value
        .as_object()
        .context("GitHub release JSON must be an object")?;
    let body = match object.get("body") {
        None | Some(serde_json::Value::Null) => return Ok(None),
        Some(serde_json::Value::String(body)) => body,
        Some(_) => anyhow::bail!("GitHub release JSON field `body` must be a string or null"),
    };
    if body.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(body.to_string()))
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
/// GitHub REST の `List releases` endpoint は成功時の HTTP status を 200 と定義するため、200 の本文を返す。
/// transport error、非 200 status、空本文、JSON 異常は文脈を付けて伝播する。release 本文が無い成功状態は、
/// 空 HTTP body ではなく endpoint が返す JSON array の空 / `body` field の空として後段で扱う。
/// 403/429 の retry は GitHub が指定する header に従う実装を持たない限り追加しない。
/// Evidence:
/// - <https://docs.github.com/en/rest/releases/releases?apiVersion=2022-11-28#list-releases>
/// - <https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api?apiVersion=2022-11-28#rate-limit-errors>
fn fetch_releases_page(api_url: &str, owner: &str, repo: &str) -> Result<String> {
    let headers: [Header<'_>; 2] = [GITHUB_ACCEPT_HEADER, GITHUB_API_VERSION_HEADER];
    let response = http_get(api_url, &headers)?;
    if response.status != reqwest::StatusCode::OK {
        anyhow::bail!(
            "GitHub releases API GET for `{owner}/{repo}` (`{api_url}`) returned HTTP status {}",
            response.status
        );
    }
    if response.body.trim().is_empty() {
        anyhow::bail!(
            "GitHub releases API GET for `{owner}/{repo}` (`{api_url}`) returned an empty HTTP body"
        );
    }
    Ok(response.body)
}

/// GitHub Releases API の配列応答を domain-local な release 表現へ翻訳する。
///
/// API 応答の JSON 構文・配列構造・必須 `tag_name` の不正は取得不能ではなく外部応答異常なので、候補なしへ
/// 縮退せず `Err` として伝播する。`name` / `body` の null・不在は GitHub API が許容する「値なし」として空文字に
/// 正規化する。
fn parse_releases(json: &str) -> Result<Vec<Release>> {
    #[derive(serde::Deserialize)]
    struct GitHubRelease {
        tag_name: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        body: Option<String>,
    }

    let releases: Vec<GitHubRelease> =
        serde_json::from_str(json).context("failed to parse GitHub releases JSON")?;
    Ok(releases
        .into_iter()
        .map(|release| Release {
            tag_name: release.tag_name,
            name: release.name.unwrap_or_default(),
            body: release.body.unwrap_or_default(),
        })
        .collect())
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

/// `notes_base` と package 名から cask 取得 URL（`Casks/<subdir>/<name>.rb`）を構築する純粋関数。subdir 規則は
/// brew tap rev 差分と同一の [`super::brew::cask_subdir`] を共有し、取得 path のずれを防ぐ。
fn resolve_cask_url(base: &str, name: &str) -> String {
    if is_cask_base(base) {
        let subdir = super::brew::cask_subdir(name);
        format!("{base}{subdir}/{name}.rb")
    } else {
        format!("{base}{name}")
    }
}

fn parse_cask_hint(rb: &str) -> Option<String> {
    let homepage = extract_dsl_string(rb, "homepage");
    let url = extract_dsl_string(rb, "url");
    normalize_cask_hint(url.as_deref()).or(homepage).or(url)
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

/// cask `url` から、AI が探索に使いやすい安定ヒント URL を導出する。
///
/// GitHub の release asset URL（`releases/download/...`）や tag URL は、そのままだと版固有で長すぎるため
/// `https://github.com/<owner>/<repo>/releases` へ正規化する。既に `.../releases` を指す URL はそのまま使う。
/// それ以外の GitHub URL は homepage の方が安定な探索ヒントになりやすいため、ここでは採らない。
fn normalize_cask_hint(url: Option<&str>) -> Option<String> {
    url.and_then(super::wire::releases_url_from_github_url)
}

fn is_cask_base(base: &str) -> bool {
    base.ends_with("/Casks/") || base == "Casks/"
}

#[cfg(test)]
mod tests {
    //! nix eval 読み取り・URL 変換・cask URL 構築・cask ヒント抽出・GitHub ヘッダ定数と、安全 fetch client
    //! 構成（redirect 不追従・https 限定・有界）・本文上限読みを network 抜きで固定する。

    use super::*;

    /// `<temp>/<prefix>-<pid>.json` の一意パスを返すテスト補助。
    fn unique_temp_path(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}.json", std::process::id()))
    }

    #[test]
    fn http_client_builds_and_read_capped_truncates_at_limit() -> Result<()> {
        // 構築が成功し（redirect/https/timeout が矛盾しない）、共有 client を返せる。
        let _ = http_client()?;
        // 上限を超える本文は limit バイトで打ち切って読む（巨大本文を全読みしない）。
        let body = vec![b'x'; 100];
        assert_eq!(read_capped(body.as_slice(), 10)?.len(), 10);
        assert_eq!(
            read_capped([b'a', b'b', b'c'].as_slice(), MAX_RESPONSE_BYTES)?,
            "abc"
        );
        Ok(())
    }

    #[test]
    fn non_success_http_status_is_propagated_not_converted_to_missing_notes() {
        let error = require_success_status(
            "https://example.test/release-notes",
            reqwest::StatusCode::NOT_FOUND,
        )
        .expect_err("HTTP 404 を Ok(None) へ縮退してはならない");
        assert!(error.to_string().contains("404"));

        assert!(
            require_success_status(
                "https://example.test/release-notes",
                reqwest::StatusCode::OK,
            )
            .is_ok()
        );
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
        let path = unique_temp_path("dotfiles-uh-nix");
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
        let missing = std::env::temp_dir().join("dotfiles-uh-nix-missing.json");
        let _ = std::fs::remove_file(&missing);
        assert!(read_nix_versions(Some(&missing))?.is_empty());
        Ok(())
    }

    #[test]
    fn notes_source_key_is_accepted_via_alias() -> Result<()> {
        let path = unique_temp_path("dotfiles-uh-nix-alias");
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
        // github bare repo root（blob/releases-tag いずれにも当たらない）は landing page（HTML chrome）であり
        // 機械 seed に使わない → None（呼び出し側で AI 探索へ回す）。末尾 `/` 有無いずれも同様。
        assert!(resolve_nix_notes_source("https://github.com/o/r").is_none());
        assert!(resolve_nix_notes_source("https://github.com/o/r/").is_none());
    }

    #[test]
    fn landing_page_urls_are_not_used_as_mechanical_seed() {
        // サイト/リポジトリのルートは HTML chrome だけで実ノートを含まないため、機械 seed に固定せず None を返す
        // （seed 無しで AI fetch_url 探索へ回す）。atuin/docker/compose/kubectl の機械 seed が bare repo root /
        // homepage HTML（300〜390KB）になり items=0 に倒れた退行を固定する。拒否経路は出所で 2 系統に分かれる:
        // github bare repo root は github host 分岐（resolve_github_notes_source が blob/releases-tag いずれにも
        // 当たらず None）、github 以外の host ルートは is_landing_page_url（path 空 or `/` のみ）で弾く。いずれも
        // 共通の契約は「機械 seed にしない」= resolve_nix_notes_source が None。
        for url in [
            "https://github.com/atuinsh/atuin",
            "https://github.com/docker/compose",
            "https://github.com/kubernetes/kubectl",
            "https://www.docker.com/",
            "https://neovim.io",
            "https://www.php.net/",
        ] {
            assert!(
                resolve_nix_notes_source(url).is_none(),
                "landing page must not become a mechanical seed: {url}"
            );
        }
        // github bare repo root は host ルートではない（path に owner/repo を持つ）が、blob/releases-tag いずれにも
        // 当たらないため github host 分岐で None になる（is_landing_page_url の対象ではない）。
        for url in [
            "https://github.com/atuinsh/atuin",
            "https://github.com/docker/compose",
            "https://github.com/kubernetes/kubectl",
        ] {
            assert!(!is_landing_page_url(url), "{url} は github bare repo root");
        }
        // github 以外の host ルート（path 空 or `/` のみ）は is_landing_page_url で弾く。
        for url in [
            "https://www.docker.com/",
            "https://neovim.io",
            "https://www.php.net/",
        ] {
            assert!(
                is_landing_page_url(url),
                "{url} は host ルートの landing page"
            );
        }
        // document path を持つ URL は landing page ではなく Raw 取得対象（実 changelog/ノート本文）。
        for url in [
            "https://bun.sh/blog/bun-v1.3.13",
            "https://raw.githubusercontent.com/o/r/v1.2.3/CHANGELOG.md",
            "https://www.postgresql.org/docs/release/14.23/",
            "https://gitlab.com/o/r/-/raw/main/CHANGELOG.md",
        ] {
            assert!(!is_landing_page_url(url), "{url} は document path を持つ");
            assert!(
                matches!(resolve_nix_notes_source(url), Some(NotesFetchPlan::Raw(got)) if got == url),
                "document path は Raw 取得対象: {url}"
            );
        }
    }

    #[test]
    fn raw_changelog_on_allowed_host_falls_back_to_raw_fetch() {
        // github 以外の許可host が指す生 changelog（raw.githubusercontent.com / gitlab.com）は Raw で取得する。
        for url in [
            "https://raw.githubusercontent.com/o/r/main/CHANGELOG.md",
            "https://gitlab.com/o/r/-/raw/main/CHANGELOG.md",
            "https://example.com/changelog",
        ] {
            match resolve_nix_notes_source(url) {
                Some(NotesFetchPlan::Raw(got)) => assert_eq!(got, url),
                _ => panic!("expected Raw for allowed host: {url}"),
            }
        }
        // SSRF 構造的拒否（http / IP / localhost / 単一ラベル）は Raw fallback に到達せず None。
        assert!(resolve_nix_notes_source("http://github.com/o/r").is_none());
        assert!(resolve_nix_notes_source("https://169.254.169.254/latest/meta-data").is_none());
        assert!(resolve_nix_notes_source("https://localhost/changelog").is_none());
        assert!(resolve_nix_notes_source("https://intranet/changelog").is_none());
    }

    #[test]
    fn chromedriver_downloads_page_is_not_used_as_mechanical_seed() {
        assert!(
            resolve_nix_notes_source("https://developer.chrome.com/docs/chromedriver/downloads")
                .is_none()
        );
    }

    #[test]
    fn release_tag_with_slash_is_percent_encoded_in_api_url() {
        // slash 入り tag（`foo/bar`）は path segment を壊すため `%2F` へ encode してから API URL を組む。
        match resolve_nix_notes_source("https://github.com/o/r/releases/tag/foo/bar") {
            Some(NotesFetchPlan::ReleasesApi { api_url, notes_url }) => {
                assert_eq!(
                    api_url,
                    "https://api.github.com/repos/o/r/releases/tags/foo%2Fbar"
                );
                assert_eq!(notes_url, "https://github.com/o/r/releases/tag/foo/bar");
            }
            _ => panic!("expected ReleasesApi with encoded tag"),
        }
        // unreserved（`-._~`）は素のまま、空白等は encode する。
        assert_eq!(encode_tag_segment("v1.0.0-rc1"), "v1.0.0-rc1");
        assert_eq!(encode_tag_segment("a b"), "a%20b");
        assert_eq!(encode_tag_segment("x/y/z"), "x%2Fy%2Fz");
    }

    #[test]
    fn extract_release_body_distinguishes_empty_body_from_malformed_external_json() -> Result<()> {
        assert_eq!(
            extract_release_body(r#"{"body":"notes"}"#)?.as_deref(),
            Some("notes")
        );
        assert_eq!(extract_release_body(r#"{"body":"  "}"#)?, None);
        assert_eq!(extract_release_body(r#"{}"#)?, None);
        assert!(extract_release_body("not JSON").is_err());
        assert!(extract_release_body(r#"{"body": 42}"#).is_err());
        Ok(())
    }

    #[test]
    fn parse_releases_propagates_malformed_external_json() -> Result<()> {
        let releases = parse_releases(r#"[{"tag_name":"v1.2.3","name":null,"body":null}]"#)?;
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].tag_name, "v1.2.3");
        assert!(releases[0].name.is_empty());
        assert!(releases[0].body.is_empty());
        assert!(parse_releases("not JSON").is_err());
        assert!(parse_releases(r#"{"tag_name":"v1.2.3"}"#).is_err());
        assert!(parse_releases(r#"[{"tag_name":42}]"#).is_err());
        Ok(())
    }

    #[test]
    fn join_release_bodies_orders_newest_first_for_truncation_safety() {
        // 取得順（API は新しい順だが page を跨ぐと前後しうる）に依らず、semver 降順（新しい版が先頭）で連結する。
        // 下流（llm）が seed を先頭から切り詰めるため、新しい版の差分を先頭に置き末尾切り捨てで落とさない。
        let joined = join_release_bodies(vec![
            ("1.0.0".to_string(), "oldest".to_string()),
            ("1.2.0".to_string(), "newest".to_string()),
            ("1.1.0".to_string(), "middle".to_string()),
        ]);
        assert_eq!(
            joined,
            format!("newest{RELEASE_BODY_SEPARATOR}middle{RELEASE_BODY_SEPARATOR}oldest")
        );
        // 先頭が最新版本文であること（先頭からの切り詰めで最新差分が残る）。
        assert!(joined.starts_with("newest"));
        // 単一版はそのまま。
        assert_eq!(
            join_release_bodies(vec![("2.0.0".to_string(), "only".to_string())]),
            "only"
        );
    }

    /// release 1 件の JSON object を組み立てるテスト補助（tag/name/body）。
    fn release_json(tag: &str, name: &str, body: &str) -> serde_json::Value {
        serde_json::json!({ "tag_name": tag, "name": name, "body": body })
    }

    #[test]
    fn release_range_relaxes_new_when_strict_window_is_empty() -> Result<()> {
        // repo-backed パッケージで API 取得は成功するが、`(old, new]` 厳密窓に該当本文が一件も無い退行ケースを固定
        // する。old=1.5.0/new=2.0.0 に対しリリースは v1.0.0 と v3.0.0 のみ（どちらも `(1.5.0, 2.0.0]` の外）で
        // 厳密窓は空。これを `route=none seed=0` に落とさず、new 上限を外した `(1.5.0, ..)` の最新該当リリースを
        // 機械 seed に緩和する。old 境界以前の release body は混ぜない。fetch_page は
        // 単一ページ（< per_page）の固定 JSON を返す注入 seam。
        let page_json = serde_json::Value::Array(vec![
            release_json("v3.0.0", "3.0.0", "## 3.0.0\n- 新しすぎる版（窓外）"),
            release_json("v1.0.0", "1.0.0", "## 1.0.0\n- old以前の本文"),
        ])
        .to_string();
        let calls = std::cell::Cell::new(0u32);
        let fetch_page = |_url: &str| -> Result<String> {
            calls.set(calls.get() + 1);
            Ok(page_json.clone())
        };
        let notes = fetch_releases_range_with("o", "r", Some("1.5.0"), Some("2.0.0"), &fetch_page)?;
        let notes = notes.ok_or_else(|| anyhow::anyhow!("relaxed range should yield a seed"))?;
        // 緩和窓 `(1.5.0, ..)` は v3.0.0 を拾い、v1.0.0（old 以前）は除外する。
        assert!(notes.text.contains("窓外"));
        assert!(!notes.text.contains("old以前"));
        assert_eq!(notes.notes_url, "https://github.com/o/r/releases");
        // 厳密 1 回（空窓）+ 緩和 1 回 = ページ取得は 2 回（短ページなので各 1 ページで停止）。
        assert_eq!(calls.get(), 2);
        Ok(())
    }

    #[test]
    fn release_range_uses_strict_window_without_relaxing_when_nonempty() -> Result<()> {
        // 厳密窓に該当本文が在れば緩和しない（厳密 1 回のみで確定）。old=1.0.0/new=2.0.0 で v2.0.0 が該当する。
        let page_json = serde_json::Value::Array(vec![
            release_json("v2.0.0", "2.0.0", "## 2.0.0\n- feature: 新機能"),
            release_json("v1.0.0", "1.0.0", "## 1.0.0\n- 旧版"),
        ])
        .to_string();
        let calls = std::cell::Cell::new(0u32);
        let fetch_page = |_url: &str| -> Result<String> {
            calls.set(calls.get() + 1);
            Ok(page_json.clone())
        };
        let notes = fetch_releases_range_with("o", "r", Some("1.0.0"), Some("2.0.0"), &fetch_page)?;
        let notes = notes.ok_or_else(|| anyhow::anyhow!("strict range should yield a seed"))?;
        // `(1.0.0, 2.0.0]` は old 排他で v1.0.0 を含まず v2.0.0 のみ。
        assert!(notes.text.contains("新機能"));
        assert!(!notes.text.contains("旧版"));
        // 厳密窓が非空なので緩和ページ取得は踏まない（1 ページのみ）。
        assert_eq!(calls.get(), 1);
        Ok(())
    }

    #[test]
    fn release_range_propagates_api_fetch_failure_without_fallback() {
        // API 取得 failure を「本文なし」に写像せず、緩和 fetch や AI fetch へ進めずに伝播する。
        let calls = std::cell::Cell::new(0u32);
        let fetch_page = |_url: &str| -> Result<String> {
            calls.set(calls.get() + 1);
            anyhow::bail!("fixture releases transport failure")
        };
        let error = fetch_releases_range_with("o", "r", Some("1.0.0"), Some("2.0.0"), &fetch_page)
            .expect_err("API failure must be propagated");
        assert!(error.to_string().contains("transport failure"), "{error}");
        // 厳密 1 回で伝播し、緩和（取得成功の空窓にだけ許可）はしない。
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn release_range_no_old_does_not_double_fetch() -> Result<()> {
        // 初回固定（old=None）は既に最広範囲。空窓でも緩和しない（同じ範囲の二重取得を避ける）。本文不在なら None。
        let page_json =
            serde_json::Value::Array(vec![release_json("v2.0.0", "2.0.0", "")]).to_string();
        let calls = std::cell::Cell::new(0u32);
        let fetch_page = |_url: &str| -> Result<String> {
            calls.set(calls.get() + 1);
            Ok(page_json.clone())
        };
        assert!(fetch_releases_range_with("o", "r", None, Some("2.0.0"), &fetch_page)?.is_none());
        // old=None なので緩和分岐に入らず、ページ取得は厳密 1 回のみ。
        assert_eq!(calls.get(), 1);
        Ok(())
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
            format!("{base}font/font-c/font-cica.rb")
        );
    }

    #[test]
    fn parse_cask_hint_prefers_github_release_url_over_generic_homepage() {
        let rb = "cask \"bitwarden\" do\n  url \"https://github.com/bitwarden/clients/releases/download/desktop-v#{version}/Bitwarden-#{version}-universal.dmg\"\n  homepage \"https://bitwarden.com/\"\nend\n";
        assert_eq!(
            parse_cask_hint(rb).as_deref(),
            Some("https://github.com/bitwarden/clients/releases")
        );
        let rb_no_home =
            "cask \"x\" do\n  url \"https://github.com/o/r/releases/download/v1/x.zip\"\nend\n";
        assert_eq!(
            parse_cask_hint(rb_no_home).as_deref(),
            Some("https://github.com/o/r/releases")
        );
        let rb_home_only = "cask \"firefox\" do\n  url \"https://download.example/firefox.dmg\"\n  homepage \"https://www.mozilla.org/firefox/\"\nend\n";
        assert_eq!(
            parse_cask_hint(rb_home_only).as_deref(),
            Some("https://www.mozilla.org/firefox/")
        );
        assert!(
            parse_cask_hint("cask \"x\" do\n  url_template \"https://example/#{version}\"\nend\n")
                .is_none()
        );
    }

    #[test]
    fn normalize_cask_hint_only_accepts_release_urls() {
        assert_eq!(
            normalize_cask_hint(Some("https://github.com/o/r/releases")).as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            normalize_cask_hint(Some("https://github.com/o/r/releases/tag/v1.2.3")).as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            normalize_cask_hint(Some("https://github.com/o/r/releases/download/v1/x.zip"))
                .as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(normalize_cask_hint(Some("https://github.com/o/r")), None);
        assert_eq!(
            normalize_cask_hint(Some("https://github.com/o/r/issues/1")),
            None
        );
    }

    #[test]
    fn brew_notes_hint_falls_back_to_package_specific_hint_when_cask_has_no_hint() -> Result<()> {
        let temp =
            std::env::temp_dir().join(format!("dotfiles-codex-app-hint-{}", std::process::id()));
        std::fs::create_dir_all(&temp)?;
        let rb = temp.join("codex-app.rb");
        std::fs::write(&rb, "cask \"codex-app\" do\n  version \"1.0.0\"\nend\n")?;
        let base = format!("file://{}", temp.display());
        let hint = brew_notes_hint(Some(&base), "codex-app")?;
        std::fs::remove_file(&rb)?;
        std::fs::remove_dir(&temp)?;
        assert_eq!(
            hint,
            Some("https://developers.openai.com/codex/changelog".to_string())
        );
        Ok(())
    }

    #[test]
    fn brew_notes_hint_uses_official_bitwarden_release_notes_fallback() -> Result<()> {
        assert_eq!(
            brew_notes_hint(None, "bitwarden")?,
            Some("https://bitwarden.com/help/releasenotes/".to_string())
        );
        Ok(())
    }

    #[test]
    fn fetch_nix_notes_degrades_without_network_when_no_hints() -> Result<()> {
        // repo/notes_source ともに無し → curl を踏まず空（hermetic）。
        assert!(fetch_nix_notes(None, None, None, None)?.is_none());
        // 構造的に許可されない notes_source は変換段で空となり、network に到達しない。
        assert!(fetch_nix_notes(None, Some("https://localhost/changelog"), None, None)?.is_none());
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
}

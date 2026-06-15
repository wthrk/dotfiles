//! OpenAI Chat Completions（`async-openai` crate）でリリースノートを構造化変更へ抽出する LLM seam。
//!
//! リリースノートの場所は機械的に一律取得できないため、抽出は **seed ノートがあればその要約、無ければ AI
//! エージェントに適切なノートを `fetch_url` ツールで取得・読解させて要約させる**方式を取る。[`ChangeExtractor::extract`]
//! は seed の有無で 2 経路へ分岐する（[`has_sufficient_seed`]）:
//!
//! - **seed が十分にある場合**（registry 再利用 fetch 成功 or 機械解決成功）: ツールを与えず seed を根拠に
//!   **1 回だけ** structured output（strict JSON schema）で要約する。OpenAI 呼び出しは 1 回。
//! - **seed が無い/空の場合のみ**: 未知ノートを探させる tool-use エージェントループ（[`run_extraction`]）を回す。
//!
//! **crate-first**: 手組み HTTP/JSON ではなく `async-openai` の typed request/response 型を使う。リクエストは
//! [`CreateChatCompletionRequestArgs`]、ツールは [`ChatCompletionTool`]（`fetch_url`）、要約出力は
//! [`ResponseFormat::JsonSchema`]（strict）で固定し、レスポンスは typed の [`ChatCompletionResponseMessage`] から
//! 直接読む。API キーは env `OPEN_AI_API_KEY` から読み crate の [`OpenAIConfig`] へ渡す（argv に現れない）。キー
//! 未設定（ローカル等）なら抽出を skip して空（呼び出し側が version-only として記録する）。
//!
//! 一過性エラー（5xx/timeout/接続/瞬間的 rate_limit）の回復は async-openai client の指数バックオフ
//! （[`CLIENT_BACKOFF_MAX_ELAPSED`]=20 秒上限）へ一本化し、[`model_call`] 自身は追加リトライしない。バックオフを
//! 使い切っても失敗するエラーは空へ縮退する（呼び出し側が version-only として確定する）。同期の record 経路から
//! 呼ぶため、async 呼び出しは専用スレッド上の current-thread runtime でブリッジする。
//!
//! **SSRF（最重要）**: AI が要求する `fetch_url` の URL は、[`super::wire::is_allowed_url`] の構造的検査
//! （https 限定・credential 拒否・IP リテラル拒否・localhost / 単一ラベルホスト拒否）を通った公開 https のみ実行する。
//! リリースノートの所在は github に限らない（cargo は doc.rust-lang.org、iterm2 は iterm2.com 等）ため、狭いホスト
//! allowlist では制限せず、構造的に安全な公開 https へ到達できる。ただし AI が選んだ URL も必ずこの構造的検査を
//! 通し（ノート本文＝信頼境界外を無検証で fetch しない）、fetch は [`super::notes::safe_https_fetch`]（redirect
//! 不追従・https 限定・有界）を再利用する。
//!
//! 抽出の trait seam は [`ChangeExtractor`] 1 つだけで、テストはこれを fake 実装に差し替える。本物の
//! [`OpenAiExtractor`] は async-openai で OpenAI を叩く。

use std::sync::mpsc;
use std::time::Duration;

use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::error::OpenAIError;
use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs, ChatCompletionTool,
    ChatCompletionToolArgs, ChatCompletionToolType, CreateChatCompletionRequest,
    CreateChatCompletionRequestArgs, FunctionObjectArgs, ResponseFormat, ResponseFormatJsonSchema,
};
use serde::Deserialize;

use super::diff::VersionDelta;
use super::notes::{RawReleaseNotes, brew_notes_hint, safe_https_fetch};
use super::wire::{ChangeCategory, ChangeItem, is_allowed_url};
use crate::Result;

/// API キーを読む env 変数名（GitHub secret。厳密表記）。
const OPENAI_API_KEY_ENV: &str = "OPEN_AI_API_KEY";

/// 抽出に使う OpenAI モデル ID（低コストかつ JSON 出力に十分な指示追従）。
const EXTRACT_MODEL: &str = "gpt-4o-mini";

/// 出力ブレを抑える低温度。
const EXTRACT_TEMPERATURE: f32 = 0.0;

/// 生リリースノートを 1 リクエストへ載せる最大文字数（char 単位。リクエストサイズの安全側上限）。
const MAX_NOTES_CHARS: usize = 6000;
/// ノートを切り詰めた際に末尾へ付ける印。
const TRUNCATION_MARKER: &str = "\n…(truncated)";

/// async-openai client 内蔵バックオフの最大経過時間。
///
/// async-openai 0.28 の既定 backoff は max_elapsed_time=15 分で、`billing_not_active`（429,
/// type!=insufficient_quota）のような恒久エラーまで 15 分リトライし続け、record が 120 分タイムアウトする。一方で
/// 0 まで縮めると一過性失敗（5xx/接続/瞬間的 rate_limit）も一切リトライされず、ノートを取得済みのパッケージでも
/// LLM 抽出が空に落ちてカバレッジを失う。そこで **20 秒**に上限を置き、crate の指数バックオフに一過性失敗の回復を
/// 任せつつ、恒久エラーは 20 秒で打ち切る。呼び出し側の hard timeout と揃え、worker が呼び出し側より長く
/// 走り続けないようにする。
const CLIENT_BACKOFF_MAX_ELAPSED: Duration = Duration::from_secs(20);

/// 1 パッケージの OpenAI 呼び出しを同期ブリッジで待つ最大時間。
///
/// client 内蔵バックオフと同じ 20 秒で揃え、CI 全体を引きずらない 1 パッケージ上限。
///
/// record は全パッケージを逐次処理するため、1 件 90 秒でも 50 件超で 1 時間級へ膨らむ。そこで 20 秒で打ち切り、
/// 要約が間に合わないものは version-only へ縮退して run 全体の前進を優先する。
const OPENAI_HARD_TIMEOUT: Duration = Duration::from_secs(20);

/// 1 パッケージあたりの tool_call 反復（fetch → 再 request）の最大回数。
const MAX_TOOL_ITERATIONS: u32 = 3;
/// 1 ターンで実行する tool_call（fetch）の最大件数。
const MAX_TOOL_CALLS_PER_TURN: usize = 4;
/// `fetch_url` ツールの名前。
const FETCH_TOOL_NAME: &str = "fetch_url";

/// AI エージェント抽出の 1 パッケージ分の入力（信頼境界内 = eval メタ由来のヒント + 機械解決済み seed）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractRequest {
    pub(crate) name: String,
    pub(crate) old: Option<String>,
    pub(crate) new: Option<String>,
    pub(crate) repo: Option<String>,
    pub(crate) homepage: Option<String>,
    pub(crate) changelog: Option<String>,
    pub(crate) seed_notes: Option<RawReleaseNotes>,
}

impl ExtractRequest {
    /// version 差分と解決済み seed から抽出リクエストを組み立てる（cask 探索ヒントは homepage へ載せる）。
    pub(crate) fn from_delta(
        delta: &VersionDelta,
        seed: Option<RawReleaseNotes>,
        brew_homepage_hint: Option<String>,
    ) -> Self {
        let homepage = brew_homepage_hint
            .or_else(|| delta.homepage.clone())
            .map(|url| normalize_hint_url(&url).unwrap_or(url));
        let changelog = delta
            .notes_source
            .as_deref()
            .map(|url| normalize_hint_url(url).unwrap_or_else(|| url.to_string()));
        ExtractRequest {
            name: delta.name.clone(),
            old: delta.old.clone(),
            new: delta.new.clone(),
            repo: delta.repo.clone(),
            homepage,
            changelog,
            seed_notes: seed,
        }
    }
}

fn normalize_hint_url(url: &str) -> Option<String> {
    super::wire::releases_url_from_github_url(url).or_else(|| Some(url.to_string()))
}

/// AI エージェント抽出の結果（構造化変更リスト + AI が採用した取得元 URL）。
///
/// `items` が空なら抽出できなかった（取得不能・変更無し・一過性失敗を run 内リトライ後も取り切れず）ことを表し、
/// 呼び出し側（record）はそのパッケージを version-only として確定する。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ExtractOutcome {
    pub(crate) items: Vec<ChangeItem>,
    pub(crate) source_url: Option<String>,
}

/// 構造化変更抽出の唯一の trait seam（テストはこれを fake 実装に差し替える）。
///
/// `extract` は seed の有無で要約/探索を選んで構造化変更を返す。取得不能・変更無し・キー未設定（ローカル等）・
/// 一過性失敗（run 内リトライ後も未完了）はいずれも空 outcome へ縮退し、呼び出し側が version-only として確定する。
pub(crate) trait ChangeExtractor {
    /// ノートを要約/探索して構造化変更へ抽出する（採用取得元 URL も返す。取得不能は空）。
    fn extract(&self, request: &ExtractRequest) -> Result<ExtractOutcome>;
}

/// structured output（strict JSON schema）で受ける抽出結果。
///
/// 個々の変更は `serde_json::Value` のまま受け、項目単位で typed [`ExtractedItem`] へ try-deserialize する
/// （未知 category の 1 項目だけを捨て、他の有効項目は残す。array 全体を落とさない）。
#[derive(Deserialize)]
struct ExtractedChanges {
    #[serde(default)]
    changes: Vec<serde_json::Value>,
}

/// structured output の 1 変更（schema の `category`/`text`/`ref` に対応。未知 category は project 段で捨てる）。
#[derive(Deserialize)]
struct ExtractedItem {
    category: ChangeCategory,
    text: String,
    #[serde(default)]
    r#ref: Option<String>,
}

/// 1 model 呼び出しの抽象（テストは fake を注入）。typed request を受け、typed response メッセージを返す。
///
/// 一過性失敗（rate_limit/429/5xx/接続）の回復は client 内蔵バックオフに委ね、ここでは追加リトライしない。
/// それでも失敗するエラーは空レスポンス（[`ResponseMessage::default`]）へ縮退する（呼び出し側が version-only として確定する）。
type ModelCall<'a> = dyn Fn(CreateChatCompletionRequest) -> Result<ResponseMessage> + 'a;

/// model 呼び出しが返す最小レスポンス（content と tool_calls だけを取り出した typed response の射影）。
#[derive(Debug, Clone, Default)]
struct ResponseMessage {
    content: Option<String>,
    tool_calls: Vec<ChatCompletionMessageToolCall>,
}

/// OpenAI 抽出を実装する本物の extractor（`async-openai` の typed client で API を叩く）。
///
/// `client` はキーが設定されている場合のみ構築する（未設定なら抽出を skip）。
pub(crate) struct OpenAiExtractor {
    /// brew cask 探索ヒント解決の `Casks/` レイアウト基底（無ければ brew は探索ヒント無し）。
    brew_notes_base: Option<String>,
    /// async-openai client（API キー未設定なら `None` で抽出 skip）。
    client: Option<Client<OpenAIConfig>>,
}

impl OpenAiExtractor {
    /// composition root から結線する extractor を生成する。
    ///
    /// API キー（env `OPEN_AI_API_KEY`）が設定されていれば async-openai client を構築する。キーは crate の
    /// [`OpenAIConfig`] へ渡され、process argv には現れない。未設定なら client は `None`（抽出 skip → version-only）。
    pub(crate) fn new(brew_notes_base: Option<String>) -> Self {
        let client = api_key().map(|key| {
            let config = OpenAIConfig::new().with_api_key(key);
            // crate 内蔵バックオフの上限を 20 秒に置く（[`CLIENT_BACKOFF_MAX_ELAPSED`]）。一過性失敗の回復は
            // このバックオフに一本化し、model_call は追加リトライしない。
            let backoff = backoff::ExponentialBackoff {
                max_elapsed_time: Some(CLIENT_BACKOFF_MAX_ELAPSED),
                ..Default::default()
            };
            Client::with_config(config).with_backoff(backoff)
        });
        Self {
            brew_notes_base,
            client,
        }
    }

    /// brew cask の探索ヒント（homepage/url）を cask `.rb` 定義から取り出す（seed が無い brew delta 用）。
    pub(crate) fn brew_homepage_hint(&self, name: &str) -> Result<Option<String>> {
        brew_notes_hint(self.brew_notes_base.as_deref(), name)
    }
}

impl ChangeExtractor for OpenAiExtractor {
    fn extract(&self, request: &ExtractRequest) -> Result<ExtractOutcome> {
        let Some(client) = self.client.as_ref() else {
            // キー未設定（ローカル等）は空 outcome へ縮退し、呼び出し側が version-only として確定する。
            eprintln!("OpenAI extract skipped: {OPENAI_API_KEY_ENV} unset");
            return Ok(ExtractOutcome::default());
        };
        let call: &ModelCall<'_> = &|req| model_call(client, req);
        // AI が選んだ URL は wire の構造的検査（is_allowed_url）を必ず通す。狭いホスト allowlist には依存しない。
        let fetch = |url: &str| fetch_allowed_note(url);
        run_extraction(request, call, fetch)
    }
}

/// API キーを読む（未設定/空なら `None`）。
fn api_key() -> Option<String> {
    std::env::var(OPENAI_API_KEY_ENV)
        .ok()
        .filter(|k| !k.is_empty())
}

/// 1 抽出の本体（network/fetch に依存しない純粋規約。model 呼び出しと fetch を注入する）。
///
/// seed が十分なら **ツール無し 1 回**の structured-output 要約、無ければ **tool-use ループ**で AI に `fetch_url`
/// させてからノートを根拠に要約させる。いずれも最終ターンは strict JSON schema で構造化変更を返す。
fn run_extraction<F>(
    request: &ExtractRequest,
    call: &ModelCall<'_>,
    mut fetch: F,
) -> Result<ExtractOutcome>
where
    F: FnMut(&str) -> Result<Option<String>>,
{
    if has_sufficient_seed(request) {
        let messages = vec![
            system_message(&summarize_system_prompt())?,
            user_message(&summarize_user_prompt(request))?,
        ];
        let response = call(summarize_request(messages)?)?;
        let items = parse_change_items(response.content.as_deref().unwrap_or_default());
        if !items.is_empty() {
            return Ok(ExtractOutcome {
                items,
                source_url: None,
            });
        }
        // 機械 seed が在っても、本文不足・HTML 主体・版別ページの揺れで 1 回要約が空に落ちることがある。
        // その場合は seed を参考情報として保持したまま tool-use へ落とし、より適切な release notes を再探索させる。
    }

    let messages = vec![
        system_message(&agent_system_prompt())?,
        user_message(&agent_user_prompt(request))?,
    ];
    run_agent_loop(call, &mut fetch, messages, None, MAX_TOOL_ITERATIONS)
}

/// tool-use エージェントループを不変メッセージ列の再帰で回す（`loop` + 可変 push を使わない）。
///
/// 1 ターン: 現在の `messages` で tool_turn を呼び、`fetch_url` 要求が無ければ最終要約ターンへ抜ける。要求が
/// あれば assistant の tool_call メッセージ + 各 tool 結果メッセージを `messages` へ不変連結し、採用取得元
/// （最後に fetch 成功した URL）を更新して、`remaining - 1` で自己再帰する。残り回数 0 でも最終要約へ抜ける。
fn run_agent_loop<F>(
    call: &ModelCall<'_>,
    fetch: &mut F,
    messages: Vec<ChatCompletionRequestMessage>,
    adopted_source: Option<String>,
    remaining: u32,
) -> Result<ExtractOutcome>
where
    F: FnMut(&str) -> Result<Option<String>>,
{
    let response = if remaining == 0 {
        None
    } else {
        Some(call(tool_turn_request(messages.clone())?)?)
    };
    let has_fetch_request = response.as_ref().is_some_and(|response| {
        response
            .tool_calls
            .iter()
            .any(|c| c.function.name == FETCH_TOOL_NAME)
    });
    let Some(response) = response.filter(|_| has_fetch_request) else {
        return summarize_after_tools(call, messages, adopted_source);
    };
    let (turn_messages, next_source) = run_tool_turn(fetch, &response.tool_calls, adopted_source)?;
    let next_messages: Vec<ChatCompletionRequestMessage> = messages
        .into_iter()
        .chain(std::iter::once(assistant_tool_call_message(
            &response.tool_calls,
        )?))
        .chain(turn_messages)
        .collect();
    run_agent_loop(call, fetch, next_messages, next_source, remaining - 1)
}

/// 1 ターンの tool_call 群を実行し、追記すべき tool 結果メッセージ列と更新後の採用取得元を返す。
///
/// 先頭 [`MAX_TOOL_CALLS_PER_TURN`] 件だけ実際に fetch し、超過分は `skipped` 結果を返す。fetch 成功した URL は
/// 採用取得元として `adopted_source` を更新する（同一ターンで複数成功すれば最後を採る）。
fn run_tool_turn<F>(
    fetch: &mut F,
    tool_calls: &[ChatCompletionMessageToolCall],
    adopted_source: Option<String>,
) -> Result<(Vec<ChatCompletionRequestMessage>, Option<String>)>
where
    F: FnMut(&str) -> Result<Option<String>>,
{
    let executed = tool_calls.iter().take(MAX_TOOL_CALLS_PER_TURN).try_fold(
        (Vec::new(), adopted_source),
        |(results, source), call_item| -> Result<_> {
            let (result, next_source) = match tool_call_url(call_item) {
                Some(url) => match fetch(&url)? {
                    Some(text) => (truncate_notes(&text), Some(url)),
                    None => (String::from("not allowed or fetch failed"), source),
                },
                None => (String::from("unsupported tool"), source),
            };
            let message = tool_result_message(&call_item.id, &result)?;
            Ok((
                results
                    .into_iter()
                    .chain(std::iter::once(message))
                    .collect(),
                next_source,
            ))
        },
    )?;
    let (executed_messages, next_source): (Vec<ChatCompletionRequestMessage>, Option<String>) =
        executed;
    let skipped: Vec<ChatCompletionRequestMessage> = tool_calls
        .iter()
        .skip(MAX_TOOL_CALLS_PER_TURN)
        .map(|call_item| tool_result_message(&call_item.id, "skipped (too many calls)"))
        .collect::<Result<_>>()?;
    Ok((
        executed_messages.into_iter().chain(skipped).collect(),
        next_source,
    ))
}

/// tool-use ターン後の最終要約を実行し、構造化変更と採用取得元を確定する。
fn summarize_after_tools(
    call: &ModelCall<'_>,
    messages: Vec<ChatCompletionRequestMessage>,
    adopted_source: Option<String>,
) -> Result<ExtractOutcome> {
    let response = call(summarize_request(messages)?)?;
    Ok(ExtractOutcome {
        items: parse_change_items(response.content.as_deref().unwrap_or_default()),
        source_url: adopted_source,
    })
}

/// 1 model 呼び出し: async-openai client で chat completion を実行し、最小レスポンスへ射影する。
///
/// 一過性エラー（rate_limit/429/5xx/接続）の回復は client 内蔵の指数バックオフ
/// （[`CLIENT_BACKOFF_MAX_ELAPSED`]=20 秒）に委ね、ここでは追加リトライしない。
/// バックオフを使い切っても失敗するエラー（一過性・恒久いずれも）は空レスポンス（[`ResponseMessage::default`]）へ
/// 縮退し、上位の空判定で version-only として確定する。is_transient はログ分類のみに使う。
fn model_call(
    client: &Client<OpenAIConfig>,
    request: CreateChatCompletionRequest,
) -> Result<ResponseMessage> {
    match run_blocking(client.clone(), request) {
        Ok(message) => Ok(message),
        Err(error) => {
            let kind = if is_transient(&error) {
                "transient"
            } else {
                "degraded"
            };
            eprintln!("OpenAI extract {kind}: {}", error_snippet(&error));
            Ok(ResponseMessage::default())
        }
    }
}

/// async-openai の chat completion を、専用スレッド上の current-thread runtime で同期実行する。
///
/// 呼び出し元（CLI dispatch）が既に current-thread runtime 内で動くため、その場で `block_on` するとネスト panic に
/// なる。新しい OS スレッドで独立した runtime を建てて async 呼び出しを完結させ、最小レスポンス（content/tool_calls）
/// だけを受け取る。
fn run_blocking(
    client: Client<OpenAIConfig>,
    request: CreateChatCompletionRequest,
) -> std::result::Result<ResponseMessage, OpenAIError> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| {
                    OpenAIError::InvalidArgument(format!("tokio runtime build failed: {error}"))
                })?;
            let response = runtime.block_on(async move { client.chat().create(request).await })?;
            let message = response
                .choices
                .into_iter()
                .next()
                .map(|choice| ResponseMessage {
                    content: choice.message.content,
                    tool_calls: choice.message.tool_calls.unwrap_or_default(),
                })
                .unwrap_or_default();
            Ok(message)
        })();
        let _ = sender.send(result);
    });
    recv_worker_result(receiver, OPENAI_HARD_TIMEOUT)
}

fn recv_worker_result(
    receiver: mpsc::Receiver<std::result::Result<ResponseMessage, OpenAIError>>,
    timeout: Duration,
) -> std::result::Result<ResponseMessage, OpenAIError> {
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(OpenAIError::InvalidArgument(format!(
            "openai hard timeout after {}s",
            timeout.as_secs()
        ))),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(OpenAIError::InvalidArgument(
            "openai worker thread disconnected".to_string(),
        )),
    }
}

/// API 失敗が一過性（接続/5xx/タイムアウト）かを判定する（少数リトライ対象）。
fn is_transient(error: &OpenAIError) -> bool {
    match error {
        OpenAIError::Reqwest(_) | OpenAIError::StreamError(_) => true,
        OpenAIError::ApiError(api) => {
            api.code
                .as_deref()
                .is_some_and(|code| code.contains("rate_limit"))
                || api.message.contains("rate limit")
                || api.message.contains("429")
        }
        _ => false,
    }
}

fn error_snippet(error: &OpenAIError) -> String {
    const LIMIT: usize = 160;
    let text = error.to_string();
    let collapsed = text.replace(['\n', '\r'], " ");
    let head: String = collapsed.chars().take(LIMIT).collect();
    if collapsed.chars().count() > LIMIT {
        format!("{head}…")
    } else {
        head
    }
}

// ---- typed リクエストビルダ ----

fn system_message(content: &str) -> Result<ChatCompletionRequestMessage> {
    Ok(ChatCompletionRequestSystemMessageArgs::default()
        .content(content)
        .build()?
        .into())
}

fn user_message(content: &str) -> Result<ChatCompletionRequestMessage> {
    Ok(ChatCompletionRequestUserMessageArgs::default()
        .content(content)
        .build()?
        .into())
}

/// AI の tool_call 要求をそのまま会話履歴へ載せる assistant メッセージ。
fn assistant_tool_call_message(
    tool_calls: &[ChatCompletionMessageToolCall],
) -> Result<ChatCompletionRequestMessage> {
    Ok(ChatCompletionRequestAssistantMessageArgs::default()
        .tool_calls(tool_calls.to_vec())
        .build()?
        .into())
}

fn tool_result_message(tool_call_id: &str, content: &str) -> Result<ChatCompletionRequestMessage> {
    Ok(ChatCompletionRequestToolMessageArgs::default()
        .tool_call_id(tool_call_id)
        .content(content)
        .build()?
        .into())
}

/// tool-use ターンのリクエスト（`fetch_url` ツールを与え、要約 or fetch を AI に選ばせる）。
fn tool_turn_request(
    messages: Vec<ChatCompletionRequestMessage>,
) -> Result<CreateChatCompletionRequest> {
    Ok(CreateChatCompletionRequestArgs::default()
        .model(EXTRACT_MODEL)
        .temperature(EXTRACT_TEMPERATURE)
        .messages(messages)
        .tools(vec![fetch_url_tool()?])
        .build()?)
}

/// 最終要約ターンのリクエスト（strict JSON schema の structured output で構造化変更だけを返させる）。
fn summarize_request(
    messages: Vec<ChatCompletionRequestMessage>,
) -> Result<CreateChatCompletionRequest> {
    Ok(CreateChatCompletionRequestArgs::default()
        .model(EXTRACT_MODEL)
        .temperature(EXTRACT_TEMPERATURE)
        .messages(messages)
        .response_format(change_items_response_format())
        .build()?)
}

/// AI へ与える `fetch_url` ツール定義（公開 https を取得できること・github 外も可であることを description で明示）。
fn fetch_url_tool() -> Result<ChatCompletionTool> {
    Ok(ChatCompletionToolArgs::default()
        .r#type(ChatCompletionToolType::Function)
        .function(
            FunctionObjectArgs::default()
                .name(FETCH_TOOL_NAME)
                .description(
                    "指定した https URL の本文を取得して返す。GitHub に限らず、構造的に安全な公開 https URL \
（プロジェクト公式サイトの changelog ページ・別リポジトリの releases・ドキュメントサイト等）を取得できる。\
取得不能な URL（http や内部アドレス等の安全でない URL を含む）は『not allowed or fetch failed』を返す。\
リリースノートや changelog の取得に使う。",
                )
                .parameters(serde_json::json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["url"],
                    "properties": { "url": { "type": "string", "description": "取得する https URL" } }
                }))
                .build()?,
        )
        .build()?)
}

/// structured output の strict JSON schema（change_items の閉集合 category と text/ref）。
fn change_items_response_format() -> ResponseFormat {
    let schema = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["changes"],
        "properties": {
            "changes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["category", "text", "ref"],
                    "properties": {
                        "category": { "type": "string", "enum": ["breaking", "security", "feature", "fix", "deprecation", "default-change"] },
                        "text": { "type": "string" },
                        "ref": { "type": ["string", "null"] }
                    }
                }
            }
        }
    });
    ResponseFormat::JsonSchema {
        json_schema: ResponseFormatJsonSchema {
            description: None,
            name: "release_changes".to_string(),
            strict: Some(true),
            schema: Some(schema),
        },
    }
}

// ---- プロンプト ----

const EXTRACT_CONTRACT_PROMPT: &str = "\
含めるのは次のカテゴリの変更だけです: 破壊的変更(breaking)、セキュリティ修正(security)、\
新機能(feature)、重要なバグ修正(fix)、非推奨化・削除(deprecation)、デフォルト挙動変更(default-change)。\
除外するもの: 内部リファクタリング、CI/ビルド変更、依存パッケージの単純な bump、ドキュメント/typo 修正、宣伝。\
各変更は category と簡潔な日本語 1 行の text を持ちます。ref はノート本文に現れた https の URL のみ、無ければ省略します。\
根拠となる変更が無ければ空の配列を返します。最終出力は指定された JSON スキーマに厳密に従ってください。";

const AGENT_PROMPT_PREFIX: &str = "\
あなたはソフトウェアのリリースノートを調査し、利用者に意味のある変更だけを抽出するエージェントです。\
与えられたパッケージ名・更新前後バージョン・ヒント URL（homepage / リポジトリ / changelog）を手がかりに、\
fetch_url ツールを使って適切なリリースノート/changelog を自分で取得して読んでください。\
fetch_url には構造的に安全な公開 https URL を渡せます（GitHub に限りません。http や内部アドレス等の安全でない \
URL は取得できません）。GitHub のパッケージなら releases ページや changelog ファイルの URL を組み立てて取得すると \
有効です。リリースノート/changelog が GitHub に無い、または『changelog は別の場所へ移動した』等のポインタしか \
無い場合は、その実際の所在（プロジェクト公式サイトの changelog ページ、別リポジトリの releases、ドキュメント \
サイト等）を自分で組み立てて fetch し、実ノート本文を取得してから抽出してください。https の公開 URL なら GitHub \
以外でも取得できます。十分なノートを読み終えたら、取得した実際のノート本文だけを根拠に変更を抽出してください。\
取得できなかった内容や本文に書かれていない内容を創作してはいけません。";

const SUMMARIZE_PROMPT_PREFIX: &str = "\
あなたはソフトウェアのリリースノートを要約し、利用者に意味のある変更だけを抽出するエージェントです。\
与えられたパッケージ名・更新前後バージョンと、本文として提示された参考リリースノートだけを根拠にしてください。\
外部取得（fetch）の手段はありません。提示された本文以外を取得したり参照したりすることはできません。\
提示された本文に書かれていない内容や、本文から確認できない内容を創作してはいけません。";

fn agent_system_prompt() -> String {
    format!("{AGENT_PROMPT_PREFIX}{EXTRACT_CONTRACT_PROMPT}")
}

fn summarize_system_prompt() -> String {
    format!("{SUMMARIZE_PROMPT_PREFIX}{EXTRACT_CONTRACT_PROMPT}")
}

fn user_prompt_header(request: &ExtractRequest) -> Vec<String> {
    let old = request.old.as_deref().unwrap_or("(なし)");
    let new = request.new.as_deref().unwrap_or("(なし)");
    let repo_lines = request
        .repo
        .as_deref()
        .filter(|s| !s.is_empty())
        .into_iter()
        .flat_map(|repo| {
            [
                format!("GitHub リポジトリ: {repo}"),
                format!("releases ページ: https://github.com/{repo}/releases"),
            ]
        });
    let homepage_line = request
        .homepage
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|homepage| format!("homepage: {homepage}"));
    let changelog_line = request
        .changelog
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|changelog| format!("changelog: {changelog}"));
    std::iter::once(format!("パッケージ: {}", request.name))
        .chain(std::iter::once(format!("更新: {old} → {new}")))
        .chain(repo_lines)
        .chain(homepage_line)
        .chain(changelog_line)
        .collect()
}

fn agent_user_prompt(request: &ExtractRequest) -> String {
    let tail = match request.seed_notes.as_ref() {
        Some(seed) => vec![
            String::from(
                "参考として機械取得したノート（不完全な場合があるため fetch_url で補ってよい）:",
            ),
            truncate_notes(&seed.text),
        ],
        None => vec![String::from(
            "fetch_url で適切なリリースノートを取得してから抽出してください。",
        )],
    };
    user_prompt_header(request)
        .into_iter()
        .chain(tail)
        .collect::<Vec<_>>()
        .join("\n")
}

fn summarize_user_prompt(request: &ExtractRequest) -> String {
    let tail = match request.seed_notes.as_ref() {
        Some(seed) => vec![
            String::from(
                "以下の参考リリースノート本文だけを根拠に変更を抽出してください（本文以外は取得・参照できません）:",
            ),
            truncate_notes(&seed.text),
        ],
        None => vec![String::from(
            "参考リリースノートは提示されていません。根拠となる本文が無いため、空の配列を返してください。",
        )],
    };
    user_prompt_header(request)
        .into_iter()
        .chain(tail)
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- パース・補助 ----

/// structured output の content（JSON）を change_item 列へパースする（未知 category はその項目を捨てる）。
fn parse_change_items(content: &str) -> Vec<ChangeItem> {
    let extracted: ExtractedChanges = match serde_json::from_str(content) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    extracted
        .changes
        .into_iter()
        .filter_map(|value| serde_json::from_value::<ExtractedItem>(value).ok())
        .map(|item| ChangeItem {
            category: item.category,
            text: item.text,
            ref_url: item.r#ref,
        })
        .collect()
}

/// 構造的に安全な公開 https URL だけを安全 fetch で取得する（SSRF 構造的検査 + 安全 fetch の合成）。
///
/// 狭いホスト allowlist には依存せず、[`is_allowed_url`]（https 限定・credential 拒否・IP リテラル拒否・
/// localhost / 単一ラベルホスト拒否）が通れば取得する。これにより github 外のノート所在（doc.rust-lang.org・
/// iterm2.com 等）へも到達でき、かつ AI が選んだ URL も必ずこの構造的検査を素通りさせない。
fn fetch_allowed_note(url: &str) -> Result<Option<String>> {
    if !is_allowed_url(url) {
        return Ok(None);
    }
    safe_https_fetch(url)
}

fn tool_call_url(call: &ChatCompletionMessageToolCall) -> Option<String> {
    if call.function.name != FETCH_TOOL_NAME {
        return None;
    }
    let args: serde_json::Value = serde_json::from_str(&call.function.arguments).ok()?;
    args.get("url")?.as_str().map(str::to_string)
}

/// seed ノートが要約の根拠に足る非空テキストを持つかを判定する純粋関数。
fn has_sufficient_seed(request: &ExtractRequest) -> bool {
    request
        .seed_notes
        .as_ref()
        .is_some_and(|notes| !notes.text.trim().is_empty())
}

fn truncate_notes(notes_text: &str) -> String {
    if notes_text.chars().count() <= MAX_NOTES_CHARS {
        return notes_text.to_string();
    }
    let truncated: String = notes_text.chars().take(MAX_NOTES_CHARS).collect();
    format!("{truncated}{TRUNCATION_MARKER}")
}

#[cfg(test)]
mod tests {
    //! OpenAI 抽出の純粋部分（キー非露出・strict schema・structured output パース・tool-use の SSRF/採用 URL/単発要約）
    //! を実 network 抜きで固定する。

    use super::*;
    use crate::update_history::diff::DeltaSource;
    use async_openai::types::{ChatCompletionToolType, FunctionCall};
    use std::cell::Cell;
    use std::sync::mpsc;

    fn request_with(name: &str, repo: Option<&str>, seed: Option<&str>) -> ExtractRequest {
        ExtractRequest {
            name: name.to_string(),
            old: Some("1.0".to_string()),
            new: Some("1.1".to_string()),
            repo: repo.map(str::to_string),
            homepage: None,
            changelog: None,
            seed_notes: seed.map(|text| RawReleaseNotes {
                text: text.to_string(),
                notes_url: "https://github.com/o/r/releases".to_string(),
                refetch_url: None,
            }),
        }
    }

    fn changes_content(json: &str) -> ResponseMessage {
        ResponseMessage {
            content: Some(json.to_string()),
            tool_calls: Vec::new(),
        }
    }

    fn fetch_tool_call(id: &str, url: &str) -> ChatCompletionMessageToolCall {
        ChatCompletionMessageToolCall {
            id: id.to_string(),
            r#type: ChatCompletionToolType::Function,
            function: FunctionCall {
                name: FETCH_TOOL_NAME.to_string(),
                arguments: serde_json::json!({ "url": url }).to_string(),
            },
        }
    }

    #[test]
    fn api_key_is_not_embedded_in_request_body() -> Result<()> {
        // API キーは async-openai の OpenAIConfig が保持し、リクエスト本文（chat completion JSON）には一切
        // 現れない（Authorization は crate が HTTP ヘッダで添える＝process argv にも出ない）。
        let messages = vec![system_message("sys")?, user_message("user")?];
        let request = summarize_request(messages)?;
        let body = serde_json::to_string(&request)?;
        assert!(!body.contains("OPEN_AI_API_KEY"), "{body}");
        assert!(!body.contains("Authorization"), "{body}");
        assert!(!body.contains("Bearer"), "{body}");
        assert_eq!(OPENAI_API_KEY_ENV, "OPEN_AI_API_KEY");
        assert_eq!(EXTRACT_MODEL, "gpt-4o-mini");
        Ok(())
    }

    #[test]
    fn client_backoff_is_bounded_between_zero_and_default() {
        // 既定 15 分（record を 120 分タイムアウトさせる）でも 0（一過性失敗を回復できずカバレッジを失う）でも
        // なく、呼び出し側 hard timeout と揃った 20 秒上限であることを固定する。
        assert_eq!(CLIENT_BACKOFF_MAX_ELAPSED, Duration::from_secs(20));
    }

    #[test]
    fn openai_hard_timeout_is_bounded() {
        assert_eq!(OPENAI_HARD_TIMEOUT, Duration::from_secs(20));
    }

    #[test]
    fn recv_worker_result_times_out_without_blocking_forever() {
        let (_sender, receiver) = mpsc::sync_channel(1);
        let err = recv_worker_result(receiver, Duration::from_millis(1))
            .expect_err("worker wait must time out");
        assert!(err.to_string().contains("openai hard timeout"));
    }

    #[test]
    fn summarize_request_uses_strict_json_schema() -> Result<()> {
        // 最終要約は strict JSON schema の structured output（手組み body ではなく typed response_format）。
        let request = summarize_request(vec![user_message("u")?])?;
        match request.response_format {
            Some(ResponseFormat::JsonSchema { json_schema }) => {
                assert_eq!(json_schema.name, "release_changes");
                assert_eq!(json_schema.strict, Some(true));
                let schema = json_schema.schema.unwrap_or_default();
                let required = &schema["properties"]["changes"]["items"]["required"];
                assert_eq!(required, &serde_json::json!(["category", "text", "ref"]));
            }
            other => panic!("expected json_schema response_format, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn tool_turn_request_offers_fetch_url_tool() -> Result<()> {
        let request = tool_turn_request(vec![user_message("u")?])?;
        let tools = request.tools.unwrap_or_default();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, FETCH_TOOL_NAME);
        Ok(())
    }

    #[test]
    fn parse_change_items_filters_unknown_category_and_keeps_valid() {
        let content = r#"{"changes":[{"category":"security","text":"CVE 修正","ref":null},{"category":"bogus","text":"無効"},{"category":"feature","text":"新機能","ref":"https://x/1"}]}"#;
        let items = parse_change_items(content);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].category, ChangeCategory::Security);
        assert_eq!(items[1].category, ChangeCategory::Feature);
        assert_eq!(items[1].ref_url.as_deref(), Some("https://x/1"));
        assert!(parse_change_items("not json").is_empty());
    }

    #[test]
    fn has_sufficient_seed_requires_nonblank() {
        assert!(has_sufficient_seed(&request_with("x", None, Some("notes"))));
        assert!(!has_sufficient_seed(&request_with("x", None, Some("   "))));
        assert!(!has_sufficient_seed(&request_with("x", None, None)));
    }

    #[test]
    fn from_delta_prefers_brew_github_release_hint_over_generic_homepage() {
        let delta = VersionDelta {
            name: "bitwarden".to_string(),
            old: Some("1.0".to_string()),
            new: Some("1.1".to_string()),
            change: super::super::wire::ChangeKind::Upgraded,
            source: DeltaSource::BrewTap,
            repo: None,
            notes_source: None,
            homepage: Some("https://bitwarden.com/".to_string()),
        };
        let request = ExtractRequest::from_delta(
            &delta,
            None,
            Some("https://github.com/bitwarden/clients/releases".to_string()),
        );
        assert_eq!(
            request.homepage.as_deref(),
            Some("https://github.com/bitwarden/clients/releases")
        );
    }

    #[test]
    fn from_delta_normalizes_github_release_tag_hint() {
        let delta = VersionDelta {
            name: "skaffold".to_string(),
            old: Some("2.17.1".to_string()),
            new: Some("2.21.0".to_string()),
            change: super::super::wire::ChangeKind::Upgraded,
            source: DeltaSource::NixEval,
            repo: Some("GoogleContainerTools/skaffold".to_string()),
            notes_source: Some(
                "https://github.com/GoogleContainerTools/skaffold/releases/tag/v2.21.0".to_string(),
            ),
            homepage: Some("https://skaffold.dev/".to_string()),
        };
        let request = ExtractRequest::from_delta(&delta, None, None);
        assert_eq!(
            request.changelog.as_deref(),
            Some("https://github.com/GoogleContainerTools/skaffold/releases")
        );
    }

    #[test]
    fn agent_prompt_uses_normalized_release_hint_for_artifact_delta() {
        let request = ExtractRequest {
            name: "nix".to_string(),
            old: Some("2.34.6+1".to_string()),
            new: Some("2.34.7+1".to_string()),
            repo: None,
            homepage: Some("https://nixos.org/nix".to_string()),
            changelog: normalize_hint_url("https://github.com/NixOS/nix/releases/tag/2.34.7"),
            seed_notes: Some(RawReleaseNotes {
                text: "thin notes".to_string(),
                notes_url: "https://github.com/NixOS/nix/releases/tag/2.34.7".to_string(),
                refetch_url: None,
            }),
        };
        let prompt = agent_user_prompt(&request);
        assert!(prompt.contains("changelog: https://github.com/NixOS/nix/releases"));
        assert!(!prompt.contains("changelog: https://github.com/NixOS/nix/releases/tag/2.34.7"));
    }

    #[test]
    fn from_delta_preserves_useful_hints_for_empty_artifact_cases() {
        let cases = vec![
            (
                VersionDelta {
                    name: "docker".to_string(),
                    old: Some("29.4.0".to_string()),
                    new: Some("29.5.3".to_string()),
                    change: super::super::wire::ChangeKind::Upgraded,
                    source: DeltaSource::NixEval,
                    repo: Some("docker/cli".to_string()),
                    notes_source: None,
                    homepage: Some("https://www.docker.com/".to_string()),
                },
                None,
                Some("docker/cli"),
                Some("https://www.docker.com/"),
                None,
            ),
            (
                VersionDelta {
                    name: "go".to_string(),
                    old: Some("1.25.9".to_string()),
                    new: Some("1.25.11".to_string()),
                    change: super::super::wire::ChangeKind::Upgraded,
                    source: DeltaSource::NixEval,
                    repo: None,
                    notes_source: Some("https://go.dev/doc/devel/release#go1.25".to_string()),
                    homepage: Some("https://go.dev/".to_string()),
                },
                None,
                None,
                Some("https://go.dev/"),
                Some("https://go.dev/doc/devel/release#go1.25"),
            ),
            (
                VersionDelta {
                    name: "skaffold".to_string(),
                    old: Some("2.17.1".to_string()),
                    new: Some("2.21.0".to_string()),
                    change: super::super::wire::ChangeKind::Upgraded,
                    source: DeltaSource::NixEval,
                    repo: Some("GoogleContainerTools/skaffold".to_string()),
                    notes_source: Some(
                        "https://github.com/GoogleContainerTools/skaffold/releases/tag/v2.21.0"
                            .to_string(),
                    ),
                    homepage: Some("https://skaffold.dev/".to_string()),
                },
                None,
                Some("GoogleContainerTools/skaffold"),
                Some("https://skaffold.dev/"),
                Some("https://github.com/GoogleContainerTools/skaffold/releases"),
            ),
            (
                VersionDelta {
                    name: "bitwarden".to_string(),
                    old: Some("2026.3.1".to_string()),
                    new: Some("2026.5.0".to_string()),
                    change: super::super::wire::ChangeKind::Upgraded,
                    source: DeltaSource::BrewTap,
                    repo: None,
                    notes_source: Some("https://github.com/bitwarden/server/releases".to_string()),
                    homepage: Some("https://bitwarden.com/".to_string()),
                },
                Some("https://github.com/bitwarden/clients/releases".to_string()),
                None,
                Some("https://github.com/bitwarden/clients/releases"),
                Some("https://github.com/bitwarden/server/releases"),
            ),
        ];

        for (delta, brew_hint, repo, homepage, changelog) in cases {
            let request = ExtractRequest::from_delta(&delta, None, brew_hint);
            assert_eq!(request.repo.as_deref(), repo, "repo for {}", delta.name);
            assert_eq!(
                request.homepage.as_deref(),
                homepage,
                "homepage for {}",
                delta.name
            );
            assert_eq!(
                request.changelog.as_deref(),
                changelog,
                "changelog for {}",
                delta.name
            );
        }
    }

    #[test]
    fn agent_prompt_includes_brew_release_hint() {
        let request = ExtractRequest {
            name: "bitwarden".to_string(),
            old: Some("2026.3.1".to_string()),
            new: Some("2026.5.0".to_string()),
            repo: None,
            homepage: Some("https://github.com/bitwarden/clients/releases".to_string()),
            changelog: None,
            seed_notes: None,
        };
        let prompt = agent_user_prompt(&request);
        assert!(prompt.contains("homepage: https://github.com/bitwarden/clients/releases"));
        assert!(prompt.contains("fetch_url で適切なリリースノートを取得"));
    }

    #[test]
    fn seeded_extraction_does_one_call_and_no_source_url() -> Result<()> {
        let calls = Cell::new(0u32);
        let call: &ModelCall<'_> = &|_request| {
            calls.set(calls.get() + 1);
            Ok(changes_content(
                r#"{"changes":[{"category":"security","text":"CVE 修正","ref":null}]}"#,
            ))
        };
        let outcome = run_extraction(
            &request_with("openssl", None, Some("CVE fix")),
            call,
            |_| Ok(None),
        )?;
        assert_eq!(calls.get(), 1, "seed 要約はツール無しで 1 回だけ呼ぶ");
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(outcome.source_url, None);
        Ok(())
    }

    #[test]
    fn empty_seeded_summary_falls_back_to_tool_use() -> Result<()> {
        let model_calls = Cell::new(0u32);
        let call: &ModelCall<'_> = &|request| {
            let n = model_calls.get();
            model_calls.set(n + 1);
            match n {
                0 => {
                    assert!(
                        request.tools.is_none(),
                        "seed 経路は最初に 1 回だけ要約する"
                    );
                    Ok(changes_content(r#"{"changes":[]}"#))
                }
                1 => {
                    let tools = request.tools.clone().unwrap_or_default();
                    assert_eq!(tools.len(), 1, "空要約後は tool-use へ落ちる");
                    Ok(ResponseMessage {
                        content: None,
                        tool_calls: vec![fetch_tool_call(
                            "c1",
                            "https://github.com/docker/cli/releases",
                        )],
                    })
                }
                _ => Ok(changes_content(
                    r#"{"changes":[{"category":"fix","text":"修正","ref":null}]}"#,
                )),
            }
        };
        let outcome = run_extraction(
            &request_with("docker", Some("docker/cli"), Some("thin seed")),
            call,
            |url| {
                assert_eq!(url, "https://github.com/docker/cli/releases");
                Ok(Some("better notes".to_string()))
            },
        )?;
        assert_eq!(
            model_calls.get(),
            4,
            "空 seed 要約後に fetch と最終要約まで進む"
        );
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(
            outcome.source_url.as_deref(),
            Some("https://github.com/docker/cli/releases")
        );
        Ok(())
    }

    #[test]
    fn agent_loop_fetches_then_summarizes_and_records_adopted_source() -> Result<()> {
        let model_calls = Cell::new(0u32);
        let call: &ModelCall<'_> = &|_request| {
            let n = model_calls.get();
            model_calls.set(n + 1);
            if n == 0 {
                Ok(ResponseMessage {
                    content: None,
                    tool_calls: vec![fetch_tool_call(
                        "c1",
                        "https://github.com/neovim/neovim/releases",
                    )],
                })
            } else {
                Ok(changes_content(
                    r#"{"changes":[{"category":"feature","text":"新機能","ref":null}]}"#,
                ))
            }
        };
        let outcome = run_extraction(
            &request_with("neovim", Some("neovim/neovim"), None),
            call,
            |url| {
                assert_eq!(url, "https://github.com/neovim/neovim/releases");
                Ok(Some("notes body".to_string()))
            },
        )?;
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(
            outcome.source_url.as_deref(),
            Some("https://github.com/neovim/neovim/releases")
        );
        Ok(())
    }

    #[test]
    fn agent_loop_returns_empty_when_no_notes_found() -> Result<()> {
        let call: &ModelCall<'_> = &|_request| Ok(changes_content(r#"{"changes":[]}"#));
        let outcome = run_extraction(&request_with("x", None, None), call, |_| Ok(None))?;
        assert!(outcome.items.is_empty());
        assert_eq!(outcome.source_url, None);
        Ok(())
    }

    #[test]
    fn fetch_allowed_note_blocks_structurally_unsafe_url() -> Result<()> {
        // 構造的に安全でない URL（http / localhost / IP リテラル / 単一ラベルホスト / credential）は
        // fetch せず None（hermetic）。狭いホスト allowlist ではなく is_allowed_url の構造的検査で塞ぐ。
        assert!(fetch_allowed_note("http://example.com/x")?.is_none());
        assert!(fetch_allowed_note("https://localhost/x")?.is_none());
        assert!(fetch_allowed_note("https://127.0.0.1/x")?.is_none());
        assert!(fetch_allowed_note("https://169.254.169.254/latest/meta-data")?.is_none());
        assert!(fetch_allowed_note("https://intranet/x")?.is_none());
        assert!(fetch_allowed_note("https://user:pass@github.com/x")?.is_none());
        assert!(fetch_allowed_note("not a url")?.is_none());
        Ok(())
    }

    #[test]
    fn truncate_notes_caps_length() {
        let long = "x".repeat(MAX_NOTES_CHARS + 100);
        let t = truncate_notes(&long);
        assert!(t.ends_with(TRUNCATION_MARKER));
        assert_eq!(truncate_notes("short"), "short");
    }

    #[test]
    fn truncate_notes_keeps_head_so_newest_release_survives() {
        // notes.rs は seed を新しい版が先頭になるよう連結する。truncate_notes は先頭 MAX_NOTES_CHARS を残すため、
        // 巨大な版区間でも最新版差分（先頭）が必ず残る（末尾切り捨てで肝心の最新差分を落とさない）。
        let newest = "NEWEST_RELEASE_NOTES ".repeat(10);
        let filler = "x".repeat(MAX_NOTES_CHARS * 2);
        let seed = format!("{newest}{filler}");
        let truncated = truncate_notes(&seed);
        assert!(truncated.starts_with("NEWEST_RELEASE_NOTES"));
        assert_eq!(
            truncated.chars().count(),
            MAX_NOTES_CHARS + TRUNCATION_MARKER.chars().count()
        );
    }

    #[test]
    fn large_seed_summarizes_without_collapsing_to_empty() -> Result<()> {
        // atuin 相当の「巨大な版区間 seed」を与えても、seed 経路（ツール無し 1 回要約）で抽出が空に倒れず
        // items>=1 を返すことを決定論で固定する（過大入力で空応答に倒れる退行を検知）。model_call は注入の fake。
        let huge_seed = "## v18.16.1\n- feature: 新機能\n".repeat(5000); // ~6 万行・MAX_NOTES_CHARS 超
        let calls = Cell::new(0u32);
        let call: &ModelCall<'_> = &|request| {
            calls.set(calls.get() + 1);
            // 送信メッセージは MAX_NOTES_CHARS で切り詰め済み（過大本文を丸ごと送らない）こと自体は
            // truncate_notes 側で固定。ここでは seed 経路が 1 回要約し非空 items を返すことを確認する。
            assert!(request.tools.is_none(), "seed 経路はツールを与えない");
            Ok(changes_content(
                r#"{"changes":[{"category":"feature","text":"新機能","ref":null}]}"#,
            ))
        };
        let outcome = run_extraction(
            &request_with("atuin", Some("atuinsh/atuin"), Some(&huge_seed)),
            call,
            |_| Ok(None),
        )?;
        assert_eq!(calls.get(), 1);
        assert_eq!(outcome.items.len(), 1);
        Ok(())
    }
}

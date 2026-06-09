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
//! GitHub Models 時代の無料枠レート制限機械（ペーシング/予算/多段バックオフ）は持たない。一過性エラー
//! （5xx/timeout/接続）は run 内で少数リトライ（[`MAX_TRANSIENT_RETRIES`]）して吸収し、取り切れなければ空へ縮退する
//! （呼び出し側が version-only として確定する。夜をまたいで再試行しない）。同期の record 経路から呼ぶため、async
//! 呼び出しは専用スレッド上の current-thread runtime でブリッジする。
//!
//! **SSRF（最重要）**: AI が要求する `fetch_url` の URL は、eval メタ由来（信頼境界内）のヒント host だけから
//! 組み立てた許可ホスト集合に host が一致する https のみ実行する。ノート本文（信頼境界外）から得た URL を無検証で
//! fetch しない。fetch は [`super::notes::safe_https_fetch`]（redirect 不追従・https 限定・有界）を再利用する。
//!
//! 抽出の trait seam は [`ChangeExtractor`] 1 つだけで、テストはこれを fake 実装に差し替える。本物の
//! [`OpenAiExtractor`] は async-openai で OpenAI を叩く。

use std::collections::BTreeSet;

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
use super::wire::{ChangeCategory, ChangeItem, allowed_fetch_hosts, fetch_host_allowed};
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

/// 一過性エラー（5xx/接続）に対する 1 リクエスト単位の再試行回数（無料枠ペーシングではなく素直な少数リトライ）。
const MAX_TRANSIENT_RETRIES: u32 = 2;

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
        ExtractRequest {
            name: delta.name.clone(),
            old: delta.old.clone(),
            new: delta.new.clone(),
            repo: delta.repo.clone(),
            homepage: brew_homepage_hint.or_else(|| delta.homepage.clone()),
            changelog: delta.notes_source.clone(),
            seed_notes: seed,
        }
    }
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
/// 一過性失敗（rate_limit/429/5xx/接続）は run 内で少数リトライし、取り切れなければ空レスポンス
/// （[`ResponseMessage::default`]）へ縮退する（呼び出し側が version-only として確定する。夜をまたいで再試行しない）。
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
            Client::with_config(config)
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
        let allowed_hosts = allowed_fetch_hosts(
            request.repo.as_deref(),
            request.homepage.as_deref(),
            request.changelog.as_deref(),
        );
        let call: &ModelCall<'_> = &|req| model_call(client, req);
        let fetch = |url: &str| fetch_allowed_note(url, &allowed_hosts);
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
        return Ok(ExtractOutcome {
            items: parse_change_items(response.content.as_deref().unwrap_or_default()),
            source_url: None,
        });
    }

    let mut messages = vec![
        system_message(&agent_system_prompt())?,
        user_message(&agent_user_prompt(request))?,
    ];
    let mut adopted_source: Option<String> = None;
    for _ in 0..MAX_TOOL_ITERATIONS {
        let response = call(tool_turn_request(messages.clone())?)?;
        let fetch_calls: Vec<&ChatCompletionMessageToolCall> = response
            .tool_calls
            .iter()
            .filter(|c| c.function.name == FETCH_TOOL_NAME)
            .collect();
        if fetch_calls.is_empty() {
            break;
        }
        messages.push(assistant_tool_call_message(&response.tool_calls)?);
        for call_item in response.tool_calls.iter().take(MAX_TOOL_CALLS_PER_TURN) {
            let result = match tool_call_url(call_item) {
                Some(url) => match fetch(&url)? {
                    Some(text) => {
                        adopted_source = Some(url);
                        truncate_notes(&text)
                    }
                    None => String::from("not allowed or fetch failed"),
                },
                None => String::from("unsupported tool"),
            };
            messages.push(tool_result_message(&call_item.id, &result)?);
        }
        for call_item in response.tool_calls.iter().skip(MAX_TOOL_CALLS_PER_TURN) {
            messages.push(tool_result_message(
                &call_item.id,
                "skipped (too many calls)",
            )?);
        }
    }
    let response = call(summarize_request(messages)?)?;
    Ok(ExtractOutcome {
        items: parse_change_items(response.content.as_deref().unwrap_or_default()),
        source_url: adopted_source,
    })
}

/// 1 model 呼び出し: async-openai client で chat completion を実行し、最小レスポンスへ射影する。
///
/// 一過性エラー（rate_limit/429/5xx/接続）は run 内で少数リトライ（[`MAX_TRANSIENT_RETRIES`]）して吸収し、取り
/// 切れなければ空レスポンス（[`ResponseMessage::default`]）へ縮退する。恒久失敗（不正リクエスト等）も同様に空
/// レスポンスへ倒す。いずれも上位の空判定で version-only として確定する（夜をまたいで再試行しない）。
fn model_call(
    client: &Client<OpenAIConfig>,
    request: CreateChatCompletionRequest,
) -> Result<ResponseMessage> {
    let mut attempt = 0;
    loop {
        match run_blocking(client.clone(), request.clone()) {
            Ok(message) => return Ok(message),
            Err(error) if is_transient(&error) && attempt < MAX_TRANSIENT_RETRIES => {
                attempt += 1;
            }
            Err(error) if is_transient(&error) => {
                // run 内リトライ後も一過性失敗 → 空レスポンスへ縮退（version-only 確定）。
                eprintln!("OpenAI extract transient: {}", error_snippet(&error));
                return Ok(ResponseMessage::default());
            }
            Err(error) => {
                // 恒久失敗（不正リクエスト等）→ 空レスポンスへ縮退（version-only 確定）。
                eprintln!("OpenAI extract degraded: {}", error_snippet(&error));
                return Ok(ResponseMessage::default());
            }
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
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| {
                        OpenAIError::InvalidArgument(format!("tokio runtime build failed: {error}"))
                    })?;
                let response =
                    runtime.block_on(async move { client.chat().create(request).await })?;
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
            })
            .join()
            .unwrap_or_else(|_| {
                Err(OpenAIError::InvalidArgument(
                    "openai worker thread panicked".to_string(),
                ))
            })
    })
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

/// AI へ与える `fetch_url` ツール定義（許可ドメインの https のみ取得できることを description で明示する）。
fn fetch_url_tool() -> Result<ChatCompletionTool> {
    Ok(ChatCompletionToolArgs::default()
        .r#type(ChatCompletionToolType::Function)
        .function(
            FunctionObjectArgs::default()
                .name(FETCH_TOOL_NAME)
                .description(
                    "指定した https URL の本文を取得して返す。許可されたドメイン（パッケージの homepage / \
リポジトリ / GitHub 公式）の https URL のみ取得でき、許可外は『not allowed』を返す。リリースノートや \
changelog の取得に使う。",
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
fetch_url には許可されたドメインの https URL だけを渡せます（許可外は『not allowed』が返ります）。\
GitHub のパッケージなら releases ページや changelog ファイルの URL を組み立てて取得すると有効です。\
十分なノートを読み終えたら、取得した実際のノート本文だけを根拠に変更を抽出してください。\
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
    let mut lines = vec![format!("パッケージ: {}", request.name)];
    let old = request.old.as_deref().unwrap_or("(なし)");
    let new = request.new.as_deref().unwrap_or("(なし)");
    lines.push(format!("更新: {old} → {new}"));
    if let Some(repo) = request.repo.as_deref().filter(|s| !s.is_empty()) {
        lines.push(format!("GitHub リポジトリ: {repo}"));
        lines.push(format!(
            "releases ページ: https://github.com/{repo}/releases"
        ));
    }
    if let Some(homepage) = request.homepage.as_deref().filter(|s| !s.is_empty()) {
        lines.push(format!("homepage: {homepage}"));
    }
    if let Some(changelog) = request.changelog.as_deref().filter(|s| !s.is_empty()) {
        lines.push(format!("changelog: {changelog}"));
    }
    lines
}

fn agent_user_prompt(request: &ExtractRequest) -> String {
    let mut lines = user_prompt_header(request);
    if let Some(seed) = request.seed_notes.as_ref() {
        lines.push(String::from(
            "参考として機械取得したノート（不完全な場合があるため fetch_url で補ってよい）:",
        ));
        lines.push(truncate_notes(&seed.text));
    } else {
        lines.push(String::from(
            "fetch_url で適切なリリースノートを取得してから抽出してください。",
        ));
    }
    lines.join("\n")
}

fn summarize_user_prompt(request: &ExtractRequest) -> String {
    let mut lines = user_prompt_header(request);
    match request.seed_notes.as_ref() {
        Some(seed) => {
            lines.push(String::from("以下の参考リリースノート本文だけを根拠に変更を抽出してください（本文以外は取得・参照できません）:"));
            lines.push(truncate_notes(&seed.text));
        }
        None => {
            lines.push(String::from("参考リリースノートは提示されていません。根拠となる本文が無いため、空の配列を返してください。"));
        }
    }
    lines.join("\n")
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

/// 許可 host 集合に属する https URL だけを安全 fetch で取得する（SSRF 検査 + 安全 fetch の合成）。
fn fetch_allowed_note(url: &str, allowed_hosts: &BTreeSet<String>) -> Result<Option<String>> {
    if !fetch_host_allowed(url, allowed_hosts) {
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
    use async_openai::types::{ChatCompletionToolType, FunctionCall};
    use std::cell::Cell;

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
    fn fetch_allowed_note_blocks_disallowed_host() -> Result<()> {
        let hosts = allowed_fetch_hosts(None, Some("https://neovim.io/"), None);
        // 許可外 host は fetch せず None（hermetic）。
        assert!(fetch_allowed_note("https://evil.example/x", &hosts)?.is_none());
        Ok(())
    }

    #[test]
    fn truncate_notes_caps_length() {
        let long = "x".repeat(MAX_NOTES_CHARS + 100);
        let t = truncate_notes(&long);
        assert!(t.ends_with(TRUNCATION_MARKER));
        assert_eq!(truncate_notes("short"), "short");
    }
}

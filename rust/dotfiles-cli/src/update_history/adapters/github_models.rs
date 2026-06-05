//! `ChangeExtractPort` を GitHub Models 推論 API（`curl` プロセス）へ接続する adapter。
//!
//! 生リリースノート（信頼境界外）を GitHub Models のチャット補完へ渡し、versioned prompt と JSON 出力
//! スキーマに従って構造化変更リスト（category + text + ref）を抽出する。認証は Actions の `GITHUB_TOKEN`
//! を `Authorization: Bearer` で使い、別 secret を要求しない。`dotfiles` の async runtime 内から blocking
//! HTTP client を使わないため、リクエストは外部 `curl` への翻訳で行う。
//!
//! 縮退契約: `GITHUB_TOKEN` 未設定・API 呼び出し失敗・JSON 解析失敗・スキーマ不一致は、record を止めず
//! 空配列（version+notes_url へ縮退）へ倒す。LLM 出力は category enum の妥当性をこの adapter の deserialize
//! で機械検証し、host/長さ/件数の機械バリデートは後段の domain（`validate`）が担う。severity は LLM 出力では
//! なく category enum から domain が算出するため、LLM 出力をマージ判断に使わない（injection 耐性）。
//!
//! prompt/スキーマの versioning: 抽出契約（含む/除外・日本語 1 行・低 temperature・ノート根拠限定）を
//! [`EXTRACT_SYSTEM_PROMPT`] と [`response_format_schema`] として本 adapter 内に versioned 固定する。

use std::ffi::OsString;

use serde::Deserialize;

use crate::Result;
use crate::process::run_capture_with_stdin;
use crate::update_history::domain::wire::{ChangeCategory, ChangeItem};
use crate::update_history::ports::{ChangeExtractPort, RawReleaseNotes};

/// GitHub Models 推論エンドポイント（OpenAI 互換チャット補完）。
const GITHUB_MODELS_ENDPOINT: &str = "https://models.github.ai/inference/chat/completions";

/// 抽出に使うモデル ID。低コストかつ JSON 出力に十分な指示追従を持つモデルを固定する。
const EXTRACT_MODEL: &str = "openai/gpt-4o-mini";

/// 出力ブレを抑えるため温度は低く固定する。
const EXTRACT_TEMPERATURE: f32 = 0.0;

/// 抽出契約を固定する versioned system prompt（v1）。
///
/// 含む: 破壊的変更/セキュリティ修正/新機能/重要バグ修正/非推奨・削除/デフォルト挙動変更。
/// 除外: 内部リファクタ/CI/ビルド/依存 bump/ドキュメント/typo/宣伝。`text` は簡潔な日本語 1 行。
/// 与えた生ノートのみを根拠とし（創作禁止）、根拠が無ければ空配列。`ref` はノート内に現れた https URL のみ。
const EXTRACT_SYSTEM_PROMPT: &str = "\
あなたはソフトウェアのリリースノートから利用者に意味のある変更だけを抽出する分類器です。\
与えられたリリースノート本文だけを根拠とし、本文に書かれていない内容を創作してはいけません。\
含めるのは次のカテゴリの変更だけです: 破壊的変更(breaking)、セキュリティ修正(security)、\
新機能(feature)、重要なバグ修正(fix)、非推奨化・削除(deprecation)、デフォルト挙動変更(default-change)。\
除外するもの: 内部リファクタリング、CI/ビルド変更、依存パッケージの単純な bump、ドキュメント/typo 修正、宣伝。\
各変更は category と簡潔な日本語 1 行の text を持ちます。ref はノート本文に現れた https の URL のみ、無ければ省略します。\
根拠となる変更が無ければ空の配列を返します。出力は指定された JSON スキーマに厳密に従ってください。";

/// LLM 出力の最上位 JSON 形（`{ "changes": [...] }`）。
#[derive(Deserialize)]
struct ExtractedChanges {
    #[serde(default)]
    changes: Vec<ExtractedItem>,
}

/// LLM が返す 1 変更項目。category は enum で deserialize し、未知値は項目ごと破棄する。
#[derive(Deserialize)]
struct ExtractedItem {
    category: ChangeCategory,
    text: String,
    #[serde(default)]
    r#ref: Option<String>,
}

/// GitHub Models のチャット補完レスポンス（必要部分のみ）。
#[derive(Deserialize)]
struct ChatCompletion {
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: String,
}

/// GitHub Models 抽出を `ChangeExtractPort` 契約へ翻訳する adapter。
pub(in crate::update_history) struct GithubModelsExtractAdapter;

impl GithubModelsExtractAdapter {
    /// `GITHUB_TOKEN` を読む。未設定/空なら `None`（抽出を空へ縮退）。
    fn github_token() -> Option<String> {
        std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty())
    }

    /// チャット補完のリクエストボディ JSON を組み立てる（versioned prompt + json schema 強制）。
    fn request_body(notes_text: &str) -> Result<String> {
        let body = serde_json::json!({
            "model": EXTRACT_MODEL,
            "temperature": EXTRACT_TEMPERATURE,
            "messages": [
                { "role": "system", "content": EXTRACT_SYSTEM_PROMPT },
                { "role": "user", "content": notes_text },
            ],
            "response_format": response_format_schema(),
        });
        Ok(serde_json::to_string(&body)?)
    }

    /// curl で GitHub Models へ POST し、レスポンス本文を返す。失敗は `Err`。
    ///
    /// 認証トークンは **argv に乗せない**。`-H "Authorization: Bearer <token>"` を引数に置くと、同一 runner の
    /// プロセス一覧（`ps`）から token が読めてしまう（secret を argv/ログに残さない義務に違反する）。代わりに
    /// curl の `--config -`（stdin から設定を読む）へ `header = "Authorization: Bearer <token>"` を流し込み、
    /// token を argv にもログにも出さない。Content-Type ヘッダと本文（`-d`）は secret ではないため argv のままで
    /// よい。stdin の内容（[`auth_config`]）は curl 設定ファイル構文で、token をクォートして 1 ヘッダだけ渡す。
    fn post(token: &str, body: &str) -> Result<String> {
        let args = [
            OsString::from("--config"),
            OsString::from("-"),
            OsString::from("--fail"),
            OsString::from("--silent"),
            OsString::from("--show-error"),
            OsString::from("--proto"),
            OsString::from("=https"),
            OsString::from("-X"),
            OsString::from("POST"),
            OsString::from("-H"),
            OsString::from("Content-Type: application/json"),
            OsString::from("-d"),
            OsString::from(body.to_string()),
            OsString::from(GITHUB_MODELS_ENDPOINT),
        ];
        run_capture_with_stdin("curl", args, auth_config(token).as_bytes())
    }

    /// レスポンス本文（チャット補完 JSON）から変更項目列を取り出す。
    ///
    /// チャット補完の `choices[0].message.content` を JSON として再解析し、`changes` 配列を
    /// [`ChangeItem`] へ翻訳する。category enum の妥当性は deserialize で検証され、未知 category や
    /// 形不一致は項目破棄/空配列へ縮退する。host/長さ/件数の機械バリデートは domain 側で別途行う。
    fn parse_response(response: &str) -> Vec<ChangeItem> {
        let completion: ChatCompletion = match serde_json::from_str(response) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };
        let Some(choice) = completion.choices.into_iter().next() else {
            return Vec::new();
        };
        let extracted: ExtractedChanges = match serde_json::from_str(&choice.message.content) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };
        extracted
            .changes
            .into_iter()
            .map(|item| ChangeItem {
                category: item.category,
                text: item.text,
                ref_url: item.r#ref,
            })
            .collect()
    }
}

/// curl の `--config -`（stdin）へ流す設定行を組み立てる。token を argv に出さず Authorization ヘッダを渡す。
///
/// curl 設定ファイル構文の `header = "..."` 形式で Authorization ヘッダ 1 件だけを与える。値はダブルクォートで
/// 囲み、token 内に万一含まれうる `\` と `"` をエスケープして構文を壊さない（GitHub Actions の token は
/// 英数字主体だが、防御的にエスケープする）。この文字列は stdin 経由でのみ curl へ渡り、argv・ログには現れない。
fn auth_config(token: &str) -> String {
    let escaped = token.replace('\\', "\\\\").replace('"', "\\\"");
    format!("header = \"Authorization: Bearer {escaped}\"\n")
}

/// JSON 出力スキーマ強制の `response_format`（versioned。category enum と必須フィールドを固定）。
///
/// **strict structured output の必須要件**: GitHub Models（OpenAI 互換）の `strict: true` は、`object` の
/// 全 `properties` を `required` に列挙することを要求し、`required` に無い property があるとスキーマ自体を
/// API が拒否して抽出が丸ごと失敗する。`ref`（ノート内の任意 URL）は LLM が「無い」を返せる必要があるが、
/// strict では単純に省略できないため、`ref` を **nullable**（`type: ["string","null"]`）にしつつ
/// `required` に含める。これにより LLM は ref 無しを JSON `null` として返せ、パース側（[`ExtractedItem`]）は
/// `Option<String>` の deserialize で `null` を `None` として受ける。category/text は常に存在する必須値。
fn response_format_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": "release_changes",
            "strict": true,
            "schema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["changes"],
                "properties": {
                    "changes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            // strict: 全 property を required に列挙する。ref は nullable で「無し」を null 表現。
                            "required": ["category", "text", "ref"],
                            "properties": {
                                "category": {
                                    "type": "string",
                                    "enum": [
                                        "breaking", "security", "feature",
                                        "fix", "deprecation", "default-change"
                                    ]
                                },
                                "text": { "type": "string" },
                                // optional な ref は strict 下で required に含めるため nullable にする
                                // （LLM は ref 無しを null で返し、パース側で null→None を許容する）。
                                "ref": { "type": ["string", "null"] }
                            }
                        }
                    }
                }
            }
        }
    })
}

impl ChangeExtractPort for GithubModelsExtractAdapter {
    fn extract_change_items(&self, notes: &RawReleaseNotes) -> Result<Vec<ChangeItem>> {
        // GITHUB_TOKEN 未設定なら呼び出さず空へ縮退（version+notes_url へフォールバック）。
        let Some(token) = Self::github_token() else {
            return Ok(Vec::new());
        };
        let body = Self::request_body(&notes.text)?;
        // 呼び出し失敗（ネットワーク/認証/レート）も record を止めず空へ縮退する。
        match Self::post(&token, &body) {
            Ok(response) => Ok(Self::parse_response(&response)),
            Err(_) => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    //! チャット補完レスポンスからの変更項目抽出（category enum 検証・未知値破棄・空縮退）と
    //! リクエストボディ/スキーマ組み立てを、実 API を呼ばずに固定する。

    use super::{GithubModelsExtractAdapter, auth_config, response_format_schema};
    use crate::update_history::domain::wire::ChangeCategory;

    #[test]
    fn auth_config_puts_token_in_stdin_header_not_argv() {
        // P1-4 退行固定: token は curl の `--config -`（stdin）の Authorization ヘッダとして渡し、argv には
        // 一切出さない。auth_config は curl 設定構文の `header = "Authorization: Bearer <token>"` を返す。
        let config = auth_config("ghs_SECRETtoken123");
        assert_eq!(
            config,
            "header = \"Authorization: Bearer ghs_SECRETtoken123\"\n"
        );
        // curl 構文を壊しうる文字（バックスラッシュ・ダブルクォート）はエスケープする。
        let escaped = auth_config(r#"a\b"c"#);
        assert_eq!(escaped, "header = \"Authorization: Bearer a\\\\b\\\"c\"\n");
    }

    fn completion_with_content(content: &str) -> String {
        serde_json::json!({
            "choices": [ { "message": { "content": content } } ]
        })
        .to_string()
    }

    #[test]
    fn parses_valid_changes_with_enum_categories() {
        let content = serde_json::json!({
            "changes": [
                { "category": "security", "text": "CVE 修正", "ref": "https://github.com/a/b/pull/1" },
                { "category": "feature", "text": "新機能" }
            ]
        })
        .to_string();
        let items = GithubModelsExtractAdapter::parse_response(&completion_with_content(&content));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].category, ChangeCategory::Security);
        assert_eq!(
            items[0].ref_url.as_deref(),
            Some("https://github.com/a/b/pull/1")
        );
        assert_eq!(items[1].category, ChangeCategory::Feature);
    }

    #[test]
    fn unknown_category_drops_to_empty() {
        // 未知 category を含む不正スキーマは deserialize 失敗で空へ縮退する。
        let content = serde_json::json!({
            "changes": [ { "category": "marketing", "text": "宣伝" } ]
        })
        .to_string();
        let items = GithubModelsExtractAdapter::parse_response(&completion_with_content(&content));
        assert!(items.is_empty());
    }

    #[test]
    fn malformed_response_is_empty() {
        assert!(GithubModelsExtractAdapter::parse_response("not json").is_empty());
        assert!(GithubModelsExtractAdapter::parse_response("{}").is_empty());
    }

    #[test]
    fn request_body_includes_prompt_schema_and_low_temperature() -> crate::Result<()> {
        let body = GithubModelsExtractAdapter::request_body("raw notes")?;
        let value: serde_json::Value = serde_json::from_str(&body)?;
        assert_eq!(value["temperature"], 0.0);
        assert_eq!(value["response_format"]["type"], "json_schema");
        assert_eq!(value["messages"][1]["content"], "raw notes");
        Ok(())
    }

    #[test]
    fn schema_enumerates_all_change_categories() {
        let schema = response_format_schema();
        let categories = &schema["json_schema"]["schema"]["properties"]["changes"]["items"]["properties"]
            ["category"]["enum"];
        let list = categories.as_array().expect("enum array");
        assert_eq!(list.len(), 6);
        assert!(list.iter().any(|v| v == "security"));
        assert!(list.iter().any(|v| v == "default-change"));
    }

    #[test]
    fn strict_schema_marks_all_properties_required_and_ref_nullable() {
        // N1 退行固定: strict structured output は item の全 property を required に列挙することを要求する。
        // ref を required から外すと API がスキーマを拒否し抽出が丸ごと失敗するため、ref は nullable
        // （type=["string","null"]）にしつつ required へ含める。
        let schema = response_format_schema();
        assert_eq!(schema["json_schema"]["strict"], true);
        let item = &schema["json_schema"]["schema"]["properties"]["changes"]["items"];
        // 全 property（category/text/ref）が required に列挙されていること。
        let required: Vec<&str> = item["required"]
            .as_array()
            .expect("required array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert!(required.contains(&"category"), "{required:?}");
        assert!(required.contains(&"text"), "{required:?}");
        assert!(
            required.contains(&"ref"),
            "strict は ref も required に含める必要がある: {required:?}"
        );
        // ref は nullable（["string","null"]）であること。
        let ref_type = item["properties"]["ref"]["type"]
            .as_array()
            .expect("ref type array");
        assert!(ref_type.iter().any(|v| v == "string"), "{ref_type:?}");
        assert!(
            ref_type.iter().any(|v| v == "null"),
            "ref は null を返せる必要がある: {ref_type:?}"
        );
    }

    #[test]
    fn null_ref_parses_as_none() {
        // strict 下で LLM が ref 無しを JSON null で返したとき、ref_url が None になる（null→None 許容）。
        let content = serde_json::json!({
            "changes": [ { "category": "fix", "text": "バグ修正", "ref": null } ]
        })
        .to_string();
        let items = GithubModelsExtractAdapter::parse_response(&completion_with_content(&content));
        assert_eq!(items.len(), 1);
        assert!(items[0].ref_url.is_none(), "null ref は None になる");
    }
}

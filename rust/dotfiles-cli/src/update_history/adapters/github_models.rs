//! `ChangeExtractPort` を GitHub Models 推論 API（`curl` プロセス）へ接続する adapter。
//!
//! 生リリースノート（信頼境界外）を GitHub Models のチャット補完へ渡し、versioned prompt と JSON 出力
//! スキーマに従って構造化変更リスト（category + text + ref）を抽出する。認証は Actions の `GITHUB_TOKEN`
//! を `Authorization: Bearer` で使い、別 secret を要求しない。`dotfiles` の async runtime 内から blocking
//! HTTP client を使わないため、リクエストは外部 `curl` への翻訳で行う。
//!
//! 縮退契約: `GITHUB_TOKEN` 未設定・API 呼び出し失敗・JSON 解析失敗・スキーマ不一致は、record を止めず
//! 空配列（version+notes_url へ縮退）へ倒す。縮退時は **なぜ空になったか**を非致命の 1 行診断として stderr へ
//! 出す（token 未設定・HTTP 非 200・curl 失敗を CI ログで可視化するため）が、token・secret は決してログへ出さ
//! ない。LLM 出力は category enum の妥当性をこの adapter の deserialize
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

/// 生リリースノートを 1 リクエストへ載せる際の最大文字数（char 単位）。
///
/// GitHub Models の gpt-4o-mini はリクエスト本文を **最大 8000 トークン**に制限する（超過は
/// `HTTP 413 tokens_limit_reached` で全件失敗する）。リクエストは「固定 system prompt（指示）+
/// response_format スキーマ + 生ノート（user message）」の合計であり、生ノートを**保守的な char 上限**で
/// 切り詰めて 8000 トークンを確実に下回らせる（厳密なトークン計算はせず、char ベースで安全側に倒す）。
///
/// 見積もりの根拠（保守的に 1 token ≒ 3 chars と置く。日本語・記号・URL でトークン密度が上がるため厚めの
/// margin を取る。実際の英語平均は 1 token ≒ 4 chars 程度なので 3 は安全側）。
/// system prompt（[`EXTRACT_SYSTEM_PROMPT`]）≈ 370 chars ⇒ 概算 ≈ 124 token、日本語主体で密度が高くても
/// 余裕を見て ≈ 400 token と置く。response_format スキーマ（[`response_format_schema`]）≈ 600 chars ⇒
/// 概算 ≈ 200 token、余裕を見て ≈ 300 token と置く。固定メッセージ枠・role ラベル等の overhead ≈ 100 token。
/// 合算すると生ノート以外の overhead ≈ 800 token を保守的に確保する。残り予算 8000 − 800 = 7200 token を
/// 生ノートへ割り当てられるが、さらに margin を厚くして生ノートは **〜2000 token 相当**に抑える。
///
/// `MAX_NOTES_CHARS = 6000` chars は、1 token ≒ 3 chars の保守見積もりで ≈ 2000 token に相当する。よって
/// 全体は overhead 800 + ノート 2000 ≈ 2800 token となり、8000 token を大幅に下回る（多言語・記号で密度が
/// 上がっても 8000 を超えない margin を確保する）。複数パッケージは各々別呼び出しのため、1 パッケージ分の
/// ノートをこの上限内へ収めれば足りる。
const MAX_NOTES_CHARS: usize = 6000;

/// ノートを切り詰めた際に末尾へ付ける印。LLM に「与えたノートは全文でない」ことを示す。
///
/// 切り詰めても「与えたノートのみを根拠とし、無ければ空配列」というハルシネーション防止契約
/// （[`EXTRACT_SYSTEM_PROMPT`]）は維持される。印自体は短く、上限見積もりへの影響は無視できる。
const TRUNCATION_MARKER: &str = "\n…(truncated)";

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
///
/// `changes` を `Vec<serde_json::Value>` で寛容に受け、各要素は [`GithubModelsExtractAdapter::parse_response`]
/// が 1 件ずつ [`ExtractedItem`] へ try-parse する。要素を `Vec<ExtractedItem>` で一括 deserialize すると、
/// 未知 category（や不正 item）が 1 件でも混ざった時点で配列全体の deserialize が失敗し、有効項目まで
/// 含めて全 changes が空へ縮退する。要素単位で受けることで、不正項目だけを drop し有効項目を保持する。
#[derive(Deserialize)]
struct ExtractedChanges {
    #[serde(default)]
    changes: Vec<serde_json::Value>,
}

/// LLM が返す 1 変更項目。category は enum で deserialize し、未知値はその項目だけ破棄する。
///
/// 破棄は要素単位で行う（[`ExtractedChanges`] 参照）。未知 category・型不一致の item を try-parse すると
/// その 1 件だけ deserialize に失敗するため、呼び出し側はその項目を drop し有効項目を残す。
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
    ///
    /// 生ノートは送信前に [`truncate_notes`] で [`MAX_NOTES_CHARS`] 以内へ切り詰め、リクエスト本文が
    /// gpt-4o-mini の 8000 トークン上限を確実に下回るようにする（上限超過は `HTTP 413` で全件失敗するため）。
    fn request_body(notes_text: &str) -> Result<String> {
        let notes_text = truncate_notes(notes_text);
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

    /// curl で GitHub Models へ POST し、`(HTTP status, レスポンス本文)` を返す。curl 自体の失敗のみ `Err`。
    ///
    /// 認証トークンは **argv に乗せない**。`-H "Authorization: Bearer <token>"` を引数に置くと、同一 runner の
    /// プロセス一覧（`ps`）から token が読めてしまう（secret を argv/ログに残さない義務に違反する）。代わりに
    /// curl の `--config -`（stdin から設定を読む）へ `header = "Authorization: Bearer <token>"` を流し込み、
    /// token を argv にもログにも出さない。Content-Type ヘッダと本文（`-d`）は secret ではないため argv のままで
    /// よい。stdin の内容（[`auth_config`]）は curl 設定ファイル構文で、token をクォートして 1 ヘッダだけ渡す。
    ///
    /// 診断のため、HTTP エラー（4xx/5xx）でも curl を非 0 終了させない。`--fail` は HTTP エラーを curl exit へ
    /// 倒し本物の status code を握り潰す（CI ログに「なぜ空縮退したか」が残らない原因だった）ので使わない。
    /// 代わりに `--write-out '%{http_code}'` で status code をレスポンス本文の末尾へ付け、[`split_status_and_body`]
    /// で末尾の status を切り出す。`-w` を足しても Authorization は stdin の `--config -` に閉じたままで、
    /// argv・ログ・`-w` の出力いずれにも token は現れない（`%{http_code}` は数値のみ）。返り値 `Err` は curl
    /// プロセス自体の失敗（spawn 失敗・ネットワーク不達等で非 0 終了）に限る。
    fn post(token: &str, body: &str) -> Result<(u16, String)> {
        let args = [
            OsString::from("--config"),
            OsString::from("-"),
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
            // status code を本文の末尾へ付加する。token は含まれない（数値のみ）。
            OsString::from("--write-out"),
            OsString::from("%{http_code}"),
            OsString::from(GITHUB_MODELS_ENDPOINT),
        ];
        let raw = run_capture_with_stdin("curl", args, auth_config(token).as_bytes())?;
        Ok(split_status_and_body(&raw))
    }

    /// レスポンス本文（チャット補完 JSON）から変更項目列を取り出す。
    ///
    /// チャット補完の `choices[0].message.content` を JSON として再解析し、`changes` 配列を
    /// [`ChangeItem`] へ翻訳する。`changes` は要素単位で try-parse し、未知 category や型不一致の item は
    /// **その項目だけ drop** して有効項目を保持する（一括 deserialize だと不正 1 件で全 changes が空へ
    /// 縮退するため）。content 全体の JSON 解析失敗・`choices` 不在のみ空配列へ縮退する。host/長さ/件数の
    /// 機械バリデートは domain 側で別途行う。
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
            // 各 item を個別に寛容に deserialize し、未知 category/不正 item はその 1 件だけ skip する。
            .filter_map(|value| serde_json::from_value::<ExtractedItem>(value).ok())
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

/// curl の stdout（本文）+ `--write-out '%{http_code}'`（末尾 status）出力から `(status, body)` を切り出す。
///
/// curl は本文をそのまま stdout へ流し、その後ろへ `%{http_code}`（常に 3 桁の数値）を追記する。よって
/// 出力末尾の連続する ASCII 数字を status code とみなし、残りを本文とする。status の解析に失敗した場合
/// （想定外出力）は status `0` を返し、呼び出し側は HTTP エラー扱いで診断ログを出す（縮退は維持）。token は
/// この出力には現れないため、ログへ本文断片を出しても secret は漏れない（`%{http_code}` は数値のみ）。
fn split_status_and_body(raw: &str) -> (u16, String) {
    let digits_start = raw.len() - raw.chars().rev().take_while(char::is_ascii_digit).count();
    let (body, status_text) = raw.split_at(digits_start);
    let status = status_text.parse::<u16>().unwrap_or(0);
    (status, body.to_string())
}

/// 生リリースノートを gpt-4o-mini のリクエスト上限内へ収めるため [`MAX_NOTES_CHARS`] 以内へ切り詰める。
///
/// リクエスト本文（system prompt + スキーマ + 生ノート）が 8000 トークン上限を超えると GitHub Models は
/// `HTTP 413 tokens_limit_reached` を返し抽出が全件失敗する。生ノートは信頼境界外の任意長テキストなので、
/// adapter（外部 API への翻訳境界）で保守的な char 上限へ切り詰めるのが翻訳責務である（トークンの厳密計算は
/// せず char ベースで安全側に倒す。上限根拠は [`MAX_NOTES_CHARS`] のコメント参照）。
///
/// 切り詰めは **char 境界**で行い（`chars()` ベース）、multibyte 文字を途中で割らない。切り詰めた場合は末尾へ
/// [`TRUNCATION_MARKER`] を付け、LLM に全文でないことを示す（ハルシネーション防止契約は維持）。上限以内の
/// 短いノートはそのまま（印を付けず）返す。
fn truncate_notes(notes_text: &str) -> String {
    // char 数で上限判定する（byte 長ではなく文字数。multibyte の途中で切らないため）。
    if notes_text.chars().count() <= MAX_NOTES_CHARS {
        return notes_text.to_string();
    }
    let truncated: String = notes_text.chars().take(MAX_NOTES_CHARS).collect();
    format!("{truncated}{TRUNCATION_MARKER}")
}

/// 診断ログ用に本文の先頭を短く切り詰める。secret は出力に現れない前提だが、長文の垂れ流しを避けるため
/// 先頭 120 byte（char 境界優先）に制限し、改行を空白へ畳んで 1 行ログに収める。
fn body_snippet(body: &str) -> String {
    const LIMIT: usize = 120;
    let trimmed = body.trim();
    let head: String = trimmed.chars().take(LIMIT).collect();
    let collapsed = head.replace(['\n', '\r'], " ");
    if trimmed.chars().count() > LIMIT {
        format!("{collapsed}…")
    } else {
        collapsed
    }
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
    /// 生リリースノートを GitHub Models で構造化変更へ抽出する。失敗は record を止めず空配列へ縮退する。
    ///
    /// 縮退契約（[module 先頭][self] 参照）は維持しつつ、**なぜ空になったか**を非致命の 1 行診断として stderr へ
    /// 出す（CI ログで HTTP 403/401 等の握り潰しを可視化するため）。診断は: token 未設定（`skipped`）、curl
    /// プロセス失敗（`degraded: curl failed`）、HTTP 非 200（`degraded: HTTP <code>` + 本文断片）。token は
    /// いずれのログにも出さない（`-w` は数値 status のみ、本文断片は token を含まない API レスポンス）。成功時は
    /// ログしない（うるさくしない）。返り値は常に `Ok`（解析結果または空配列）で record を止めない。
    fn extract_change_items(&self, notes: &RawReleaseNotes) -> Result<Vec<ChangeItem>> {
        // GITHUB_TOKEN 未設定なら呼び出さず空へ縮退（version+notes_url へフォールバック）。未設定検知時に 1 度だけ
        // ログする（呼び出しごとではない＝この経路自体が未設定時に 1 回通る）。
        let Some(token) = Self::github_token() else {
            eprintln!("GitHub Models extract skipped: GITHUB_TOKEN unset");
            return Ok(Vec::new());
        };
        let body = Self::request_body(&notes.text)?;
        // 呼び出し失敗（ネットワーク/認証/レート）も record を止めず空へ縮退する。診断ログだけ残す。
        match Self::post(&token, &body) {
            Ok((200, response)) => Ok(Self::parse_response(&response)),
            Ok((status, response)) => {
                // HTTP エラー: status と本文断片（token 非含有）を 1 行ログ。空へ縮退。
                eprintln!(
                    "GitHub Models extract degraded: HTTP {status}: {}",
                    body_snippet(&response)
                );
                Ok(Vec::new())
            }
            Err(_) => {
                // curl プロセス自体の失敗（spawn/ネットワーク不達等）。error 本文に token は含まれないが、防御的に
                // 詳細は出さず固定文言のみログする。空へ縮退。
                eprintln!("GitHub Models extract degraded: curl failed");
                Ok(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! チャット補完レスポンスからの変更項目抽出（category enum 検証・未知値破棄・空縮退）と
    //! リクエストボディ/スキーマ組み立てを、実 API を呼ばずに固定する。

    use super::{
        GithubModelsExtractAdapter, MAX_NOTES_CHARS, TRUNCATION_MARKER, auth_config, body_snippet,
        response_format_schema, split_status_and_body, truncate_notes,
    };
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
        // 未知 category の item はその項目だけ drop される。全項目が未知なら結果は空になる。
        let content = serde_json::json!({
            "changes": [ { "category": "marketing", "text": "宣伝" } ]
        })
        .to_string();
        let items = GithubModelsExtractAdapter::parse_response(&completion_with_content(&content));
        assert!(items.is_empty());
    }

    #[test]
    fn unknown_category_item_is_skipped_without_dropping_valid_items() {
        // 退行固定: `changes` 配列に未知 category が 1 件混ざっても、配列全体の deserialize を失敗させて
        // 全 changes を空へ縮退させてはならない。不正 item はその 1 件だけ drop し、有効項目は保持する。
        let content = serde_json::json!({
            "changes": [
                { "category": "security", "text": "CVE 修正", "ref": "https://github.com/a/b/pull/1" },
                { "category": "marketing", "text": "宣伝" },
                { "category": "feature", "text": "新機能" }
            ]
        })
        .to_string();
        let items = GithubModelsExtractAdapter::parse_response(&completion_with_content(&content));
        // 有効 2 件（security, feature）が残り、未知 category の 1 件だけ落ちる。
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].category, ChangeCategory::Security);
        assert_eq!(items[1].category, ChangeCategory::Feature);
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
    fn split_status_and_body_separates_trailing_http_code() {
        // curl は本文の末尾へ `%{http_code}`（3 桁数値）を付加する。末尾の連続数字を status として切り出し、
        // 残りを本文とする。JSON 本文が `}` で終わるため数字が本文末尾と衝突しない。
        let (status, body) = split_status_and_body(r#"{"choices":[]}200"#);
        assert_eq!(status, 200);
        assert_eq!(body, r#"{"choices":[]}"#);

        // HTTP エラー status も同様に切り出せる。
        let (status, body) = split_status_and_body(r#"{"error":"forbidden"}403"#);
        assert_eq!(status, 403);
        assert_eq!(body, r#"{"error":"forbidden"}"#);
    }

    #[test]
    fn split_status_and_body_returns_zero_when_no_status() {
        // 想定外出力（status 末尾無し）は status 0（呼び出し側で HTTP エラー扱い→診断ログ→空縮退）。
        let (status, body) = split_status_and_body("not json no status");
        assert_eq!(status, 0);
        assert_eq!(body, "not json no status");
    }

    #[test]
    fn body_snippet_truncates_and_collapses_newlines() {
        // 長文は先頭 120 char に切り詰め省略記号を付ける。改行は空白へ畳んで 1 行ログに収める。
        let long = "a".repeat(200);
        let snippet = body_snippet(&long);
        assert!(snippet.ends_with('…'), "{snippet}");
        assert_eq!(snippet.chars().count(), 121); // 120 char + 省略記号
        let multiline = body_snippet("line1\nline2\r\nline3");
        assert!(!multiline.contains('\n'), "{multiline}");
        assert!(!multiline.contains('\r'), "{multiline}");
    }

    #[test]
    fn diagnostic_snippet_never_contains_token() {
        // token は API レスポンス本文に現れない（auth は stdin の `--config -` に閉じる）。仮に本文へ token 様の
        // 文字列が混ざっても、診断ログに使う body_snippet/auth_config の経路は token を別管理する。ここでは
        // auth_config（stdin 専用）が body_snippet（ログ用）と別関数であること、ログ経路が本文だけを扱うことを
        // 退行固定する: auth ヘッダ文字列は body_snippet の入力に決して渡らない。
        let response_body = r#"{"choices":[{"message":{"content":"{\"changes\":[]}"}}]}"#;
        let snippet = body_snippet(response_body);
        assert!(!snippet.contains("Authorization"), "{snippet}");
        assert!(!snippet.contains("Bearer"), "{snippet}");
        // auth_config（token を含む）はログには使わず stdin 専用であることを示す（別関数・別経路）。
        let auth = auth_config("ghs_SECRET");
        assert!(auth.contains("ghs_SECRET"));
        assert!(!snippet.contains("ghs_SECRET"));
    }

    #[test]
    fn short_notes_are_not_truncated() {
        // 上限以内の短いノートはそのまま（切り詰め印を付けず）返す。
        let notes = "短いリリースノート";
        let result = truncate_notes(notes);
        assert_eq!(result, notes);
        assert!(!result.contains(TRUNCATION_MARKER));
    }

    #[test]
    fn notes_at_exact_limit_are_not_truncated() {
        // ちょうど上限（MAX_NOTES_CHARS）のノートは切り詰めない（`<=` 境界）。
        let notes = "a".repeat(MAX_NOTES_CHARS);
        let result = truncate_notes(&notes);
        assert_eq!(result.chars().count(), MAX_NOTES_CHARS);
        assert!(!result.contains(TRUNCATION_MARKER));
    }

    #[test]
    fn over_limit_notes_are_truncated_to_max_chars_plus_marker() {
        // 退行固定: 上限超のノートは MAX_NOTES_CHARS char へ切られ、末尾へ切り詰め印が付く。
        // これにより gpt-4o-mini の 8000 トークン上限を超えず HTTP 413 を避ける。
        let notes = "a".repeat(MAX_NOTES_CHARS + 5000);
        let result = truncate_notes(&notes);
        // 本体は厳密に MAX_NOTES_CHARS char。残りは切り詰め印のみ。
        assert!(result.starts_with(&"a".repeat(MAX_NOTES_CHARS)));
        assert!(result.ends_with(TRUNCATION_MARKER));
        let marker_chars = TRUNCATION_MARKER.chars().count();
        assert_eq!(result.chars().count(), MAX_NOTES_CHARS + marker_chars);
    }

    #[test]
    fn truncation_cuts_on_char_boundary_for_multibyte() {
        // 退行固定: multibyte 文字を途中で割らない（chars() ベースで切る）。各文字は 3 byte の日本語。
        // 上限を 1 char 超える長さで切り、結果が valid UTF-8（panic せず）かつ MAX_NOTES_CHARS char
        // ＋印であることを確認する。byte 単位で切ると multibyte 境界を壊しうるが char 単位なら安全。
        let notes = "あ".repeat(MAX_NOTES_CHARS + 100);
        let result = truncate_notes(&notes);
        assert!(result.starts_with(&"あ".repeat(MAX_NOTES_CHARS)));
        assert!(result.ends_with(TRUNCATION_MARKER));
        let marker_chars = TRUNCATION_MARKER.chars().count();
        assert_eq!(result.chars().count(), MAX_NOTES_CHARS + marker_chars);
    }

    #[test]
    fn request_body_truncates_over_limit_notes() -> crate::Result<()> {
        // 退行固定: request_body は上限超ノートを user message へ載せる前に切り詰める。
        // user content の char 数が MAX_NOTES_CHARS + 印 を超えないことを確認する（HTTP 413 回避の核）。
        let long_notes = "x".repeat(MAX_NOTES_CHARS + 9000);
        let body = GithubModelsExtractAdapter::request_body(&long_notes)?;
        let value: serde_json::Value = serde_json::from_str(&body)?;
        let content = value["messages"][1]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("user content missing"))?;
        let marker_chars = TRUNCATION_MARKER.chars().count();
        assert_eq!(content.chars().count(), MAX_NOTES_CHARS + marker_chars);
        assert!(content.ends_with(TRUNCATION_MARKER));
        Ok(())
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

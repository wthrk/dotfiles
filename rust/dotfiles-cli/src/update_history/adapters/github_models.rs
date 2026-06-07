//! `ChangeExtractPort` を GitHub Models 推論 API（`curl` プロセス）へ接続する adapter。
//!
//! リリースノートの場所は機械的に一律取得できないため、抽出は **seed ノートがあればその要約、無ければ AI
//! エージェントに適切なノートを取得させて要約させる**方式を取る。`extract_change_items` は seed の有無で 2 経路へ
//! 分岐する（[`has_sufficient_seed`]）:
//!
//! - **seed が十分にある場合**（registry 再利用 fetch 成功 or 機械解決成功で seed ノートが既に取得済み）:
//!   **ツールを与えず seed を根拠に 1 回だけ** structured output（strict schema）で要約する（[`summarize_only`]）。
//!   `fetch_url` 探索をしないため **GitHub Models 呼び出しは 1 回**。registry が埋まるほど機械的に取れる/学習済みの
//!   ものが 1 回化され、GitHub Models のレート消費が回を追って実際に逓減する（利用者要件 4 が真になる）。
//!   ai-discovered 学習は行わない（探索経路でのみ更新）。
//! - **seed が無い/空の場合のみ**: 未知ノートを探させる **tool-use エージェントループ**（[`agent_loop`]）を回す。
//!   adapter は [`ExtractRequest`] のヒント（パッケージ名・old→new・homepage/repo/changelog）から、パッケージの
//!   正規ドメインに限定した fetch 許可ホスト集合を組み立て（domain の
//!   [`allowed_fetch_hosts`](crate::update_history::domain::validate::allowed_fetch_hosts)）、AI に `fetch_url`
//!   ツールを与えて会話ループを回す:
//!
//!   1. 初回 request: system prompt（抽出契約 + ハルシネーション防止）+ user（パッケージ名・old→new・ヒント URL
//!      群）+ `tools=[fetch_url]` + `tool_choice=auto`。
//!   2. レスポンスが `finish_reason: tool_calls` なら各 `fetch_url(url)` を **SSRF 検査つきで実行**し、取得テキスト
//!      （truncate 済）を `role: tool` メッセージで会話へ追加して再 request。
//!   3. AI が十分なノートを読んだら（tool_calls を出さなくなる、または反復上限）、**tools を外し response_format
//!      （strict schema）を付けた最終ターン**で構造化変更リストを返させる。
//!   4. 有界: 1 パッケージあたり tool_call 反復は最大 [`MAX_TOOL_ITERATIONS`] 回（model 呼び出しは最大
//!      `MAX_TOOL_ITERATIONS + 1` 回）。超過したら持っている材料で最終要約ターンへ移る。
//!
//! ハルシネーション防止（与えた seed の実ノートのみを根拠とし、無ければ空）は両経路で維持する
//! （[`EXTRACT_SYSTEM_PROMPT`] 契約は tools の有無に依らない）。
//!
//! 認証は Actions の `GITHUB_TOKEN` を `Authorization: Bearer` で使い、別 secret を要求しない。`dotfiles` の
//! async runtime 内から blocking HTTP client を使わないため、リクエストは外部 `curl` への翻訳で行う。
//!
//! **SSRF（最重要）**: AI が要求する `fetch_url` の URL は、eval メタ由来（信頼境界内）のヒント host だけから
//! 組み立てた許可ホスト集合に host が一致する https のみ実行する。集合外の URL は fetch せずツール結果として
//! 「not allowed」を返す（AI は別 URL を試せる）。**ノート本文（信頼境界外）から得た URL を無検証で fetch
//! しない**＝ホスト集合はパッケージごとに eval メタから組み立て、ノート内容で拡張しない。fetch は注入された
//! 安全 fetch（[`notes`](crate::update_history::adapters) の `-L` 無し・`--max-redirs 0`・`--proto =https`・https
//! のみの curl）を再利用し、取得テキストは [`MAX_NOTES_CHARS`] へ truncate、総 fetch 回数も [`MAX_TOOL_ITERATIONS`]
//! × [`MAX_TOOL_CALLS_PER_TURN`] で有界にする。
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
//!
//! レート制限（HTTP 429）対応: GitHub Models の無料枠は per-minute レート上限が低く、record は差分パッケージ
//! 分の要約を立て続けに投げるため上限へ当たりやすい。1 リクエスト単位では `HTTP 429` を受けた際に
//! [`Retry-After`](RETRY_AFTER_HEADER) ヘッダ（秒）を尊重して待機し、無ければ指数バックオフで有界回数だけ
//! 再試行する（[`GithubModelsExtractAdapter::post_with_retry`]）。複数パッケージにまたがる呼び出し列の間隔は
//! adapter が保持する呼び出し間隔状態（[`MIN_REQUEST_INTERVAL`]）で per-minute 上限内へ収まるようペースを敷く
//! （[`ChangeExtractPort`] 実装参照）。最終的に 429 が解消しなくても record は止めず空配列へ縮退する。
//!
//! 抽出フェーズ全体の wall-clock 予算（[`EXTRACT_BUDGET`]）: 1 リクエストの 429 リトライ上振れ（最大 ~80s）と
//! パッケージ間ペーシングが積み上がると、全件持続 429 のとき抽出ループ全体が record job timeout（60分）へ接近・
//! 超過しうる（超過すると後続 job（PR 起票）が止まり無人 nightly が停止する）。これを構造的に防ぐため、抽出
//! フェーズ開始時刻を起点に総時間予算を設け、超過後は残りパッケージの LLM 抽出を skip して version-only へ縮退
//! させる（[`GithubModelsExtractAdapter::extract_budget_exhausted`]、停止条件の適用は application が担う）。
//! 個々の 429 リトライ待機も deadline を跨ぐなら待たずに縮退する（[`retry_loop`] へ残予算を渡す）。

use std::cell::Cell;
use std::ffi::OsString;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::Result;
use crate::process::run_capture_with_stdin;
use crate::update_history::domain::validate::allowed_fetch_hosts;
use crate::update_history::domain::wire::{ChangeCategory, ChangeItem};
use crate::update_history::ports::{ChangeExtractPort, ExtractOutcome, ExtractRequest};
use crate::update_history::support::fetch_allowed_note;

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

/// HTTP 429（レート制限）に対する 1 リクエスト単位の最大再試行回数。
///
/// 待機を挟んで最大この回数だけ再試行し、それでも 429 が解消しなければ空配列へ縮退する（record は止めない）。
/// 有界にするのは、daily 上限など待っても解消しない 429 でリクエストが無限に粘って record job timeout（60分）を
/// 食い潰すのを防ぐため。最大 4 回 + 指数バックオフ（2+4+8+16=30s）でも 1 リクエストの上振れは約 30 秒に収まる。
const MAX_RATE_LIMIT_RETRIES: u32 = 4;

/// `Retry-After` ヘッダが無い 429 で使う指数バックオフの基準待機（最初の再試行前に待つ秒数）。
///
/// retry 回数 n（0 始まり）で `BACKOFF_BASE * 2^n` 秒待つ（2s, 4s, 8s, 16s）。`Retry-After` があればそちらを
/// 優先し、この指数バックオフは fallback として使う。
const BACKOFF_BASE: Duration = Duration::from_secs(2);

/// 指数バックオフの 1 回あたり上限。サーバが極端な `Retry-After` を返した場合も含め、1 回の待機がこれを超えない
/// ようにして 1 リクエストの総待機が読めなくなる（timeout を食い潰す）のを防ぐ。
const MAX_BACKOFF: Duration = Duration::from_secs(20);

/// パッケージ間の呼び出しに敷く最小間隔。per-minute レート上限内へ収めるためのペーシング基準。
///
/// 直前のリクエスト開始からこの間隔が経つまで次のリクエストを開始しない（[`ChangeExtractPort`] 実装が
/// adapter 保持の最終リクエスト時刻で待機を挿入する）。
///
/// 値の根拠（per-minute レート上限を確実に下回ること）: GitHub Models 無料枠の per-minute レート上限は典型的に
/// ~15 req/min と低く、CI 実走では旧値（2s ≒ 30 req/min）が上限を超過して HTTP 429 が継続した（`Retry-After: 7s`
/// 程度が返る本物のレート制限）。**8 秒**間隔は 60s ÷ 8s = **7.5 req/min** で、~15 req/min 上限の半分以下に確実に
/// 収まり 429 を回避する。直前リクエストの **開始**時刻基準で待機するため、429 リトライ待機とは二重計上されない
/// （[`pace_before_request`](GithubModelsExtractAdapter::pace_before_request)）。
///
/// 予算内であること: 差分パッケージが多くても全体は「件数 × 本間隔 + retry 上振れ」に収まる。30 件 × 8s = 240s
/// ≒ 4 分で、抽出フェーズ予算（[`EXTRACT_BUDGET`] = 35 分）・record job timeout（60 分）を大きく下回る（本間隔の
/// 拡大は予算へ十分な余裕を残す）。なお無料枠の per-minute/日次 quota が極端に低い場合は本間隔でも 429 が残りうるが、
/// その際も version+notes_url へ縮退して record success を維持する（既存契約・[`ChangeExtractPort`] 実装参照）。
const MIN_REQUEST_INTERVAL: Duration = Duration::from_secs(8);

/// 抽出フェーズ全体に与える wall-clock 予算（deadline）。超過後は残りパッケージの LLM 抽出を skip して
/// version-only へ縮退する（[`GithubModelsExtractAdapter::extract_budget_exhausted`]）。
///
/// 根拠（最悪ケース＝全件持続 429 でも record 総時間が record job timeout（60分）を割らないこと）:
/// - record job（`nightly-update.yml` の `record`）は `timeout-minutes: 60`。抽出フェーズ以外に、devShell 起動・
///   `nix build .#default`（dotfiles バイナリ）・nix/brew 版差分の取得・各パッケージのノート取得 HTTP・TOML 追記等の
///   他工程がある。これらに安全側で **~15 分**を見込む。
/// - 抽出フェーズへ与える予算をこの **35 分**に固定すると、抽出 ≤ 35 分 + 他工程 ~15 分 = ~50 分 ≤ 60 分の
///   構造的マージン（~10 分）を確保できる。
/// - 予算超過の判定はループの各抽出の **前**に行い、超過後は LLM 呼び出しを一切しない。さらに個々の 429 リトライ
///   待機も deadline を跨ぐなら待たずに縮退する（[`retry_loop`] へ残予算を渡す）ため、抽出フェーズの実 wall-clock は
///   「予算 + 進行中だった 1 リクエストの最大上振れ（~80s）」を超えない。35 分 + ~80s でも 60 分を大きく下回る。
/// - 個々の 1 リクエスト上振れ ~80s の内訳: 429 リトライ最大 4 回の待機（各 ≤ MAX_BACKOFF=20s ⇒ 最大 ~80s）+
///   curl の応答時間。pacing（2s）は次リクエスト開始前の待機で deadline 判定と直交する。
const EXTRACT_BUDGET: Duration = Duration::from_secs(35 * 60);

/// curl で取得した HTTP ヘッダ JSON から `Retry-After` を読むためのヘッダ名（小文字）。`%{header_json}` の
/// キーは小文字化されて入るため、参照側も小文字で引く。
const RETRY_AFTER_HEADER: &str = "retry-after";

/// curl の `--write-out` 末尾トレーラを本文から切り出す sentinel。本文（任意の API レスポンス）と衝突しない
/// 一意文字列を選ぶ。トレーラはこの sentinel に続いて `http_code` と `header_json` を改行区切りで持つ。
const CURL_META_SENTINEL: &str = "\n<<<DOTFILES_CURL_META>>>\n";

/// 1 パッケージあたりの tool_call 反復（fetch → 再 request）の最大回数。
///
/// AI が `fetch_url` を要求するたびに 1 反復を消費する。上限に達したら（または AI が tool_calls を出さなく
/// なったら）tools を外して response_format 強制の最終要約ターンへ移る。有界にするのは、AI が延々と fetch を
/// 要求して model 呼び出しが膨らみ、抽出フェーズ予算（[`EXTRACT_BUDGET`]）や record job timeout を食い潰すのを
/// 防ぐため。各反復は 1 model 呼び出し + 数件の fetch であり、3 反復で「初回 → fetch → 再考 → fetch → 再考」
/// 程度の探索が可能。
const MAX_TOOL_ITERATIONS: u32 = 3;

/// 1 ターンで実行する tool_call（fetch）の最大件数。
///
/// AI が 1 レスポンスで複数 `fetch_url` を並べても、実行する fetch をこの件数で打ち切る（残りは「skipped」を
/// ツール結果で返す）。1 パッケージの総 fetch 回数を [`MAX_TOOL_ITERATIONS`] × この値で有界にし、過剰 HTTP を
/// 防ぐ。
const MAX_TOOL_CALLS_PER_TURN: usize = 4;

/// `fetch_url` ツールの名前（AI が tool_call で指定し、adapter が照合する）。
const FETCH_TOOL_NAME: &str = "fetch_url";

/// 抽出契約を固定する versioned system prompt（v1）。
///
/// 含む: 破壊的変更/セキュリティ修正/新機能/重要バグ修正/非推奨・削除/デフォルト挙動変更。
/// 除外: 内部リファクタ/CI/ビルド/依存 bump/ドキュメント/typo/宣伝。`text` は簡潔な日本語 1 行。
/// 与えた生ノートのみを根拠とし（創作禁止）、根拠が無ければ空配列。`ref` はノート内に現れた https URL のみ。
const EXTRACT_SYSTEM_PROMPT: &str = "\
あなたはソフトウェアのリリースノートを調査し、利用者に意味のある変更だけを抽出するエージェントです。\
与えられたパッケージ名・更新前後バージョン・ヒント URL（homepage / リポジトリ / changelog）を手がかりに、\
fetch_url ツールを使って適切なリリースノート/changelog を自分で取得して読んでください。\
fetch_url には許可されたドメインの https URL だけを渡せます（許可外は『not allowed』が返ります）。\
GitHub のパッケージなら releases ページや changelog ファイルの URL を組み立てて取得すると有効です。\
十分なノートを読み終えたら、取得した実際のノート本文だけを根拠に変更を抽出してください。\
取得できなかった内容や本文に書かれていない内容を創作してはいけません。\
含めるのは次のカテゴリの変更だけです: 破壊的変更(breaking)、セキュリティ修正(security)、\
新機能(feature)、重要なバグ修正(fix)、非推奨化・削除(deprecation)、デフォルト挙動変更(default-change)。\
除外するもの: 内部リファクタリング、CI/ビルド変更、依存パッケージの単純な bump、ドキュメント/typo 修正、宣伝。\
各変更は category と簡潔な日本語 1 行の text を持ちます。ref はノート本文に現れた https の URL のみ、無ければ省略します。\
根拠となる変更が無ければ空の配列を返します。最終出力は指定された JSON スキーマに厳密に従ってください。";

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
    /// `tool_calls` / `stop` 等。エージェントループはこれで「AI がツールを呼びたいか」を判定する。
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: String,
    /// AI が要求した tool_call（`fetch_url(url)`）。無ければ空。エージェントループがこれを実行する。
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

/// AI が返す 1 件の tool_call（OpenAI 互換）。`id` は対応する `role: tool` 応答へ紐付ける。
#[derive(Deserialize)]
struct ToolCall {
    #[serde(default)]
    id: String,
    function: ToolCallFunction,
}

/// tool_call の関数指定。`name` は [`FETCH_TOOL_NAME`] と照合、`arguments` は JSON 文字列（`{"url": "..."}`）。
#[derive(Deserialize)]
struct ToolCallFunction {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: String,
}

/// GitHub Models 抽出を `ChangeExtractPort` 契約へ翻訳する adapter。
///
/// `last_request_at` は直前にリクエストを開始した時刻を保持し、複数パッケージにまたがる呼び出し列へ
/// [`MIN_REQUEST_INTERVAL`] のペーシングを敷くために使う（per-minute レート上限内へ収めるため）。
/// `extract_phase_start` は抽出フェーズ開始時刻（最初の予算判定/抽出のいずれか早い方で確定）を保持し、
/// [`EXTRACT_BUDGET`] の wall-clock 予算超過を判定するために使う（全件持続 429 で抽出が record job timeout を
/// 食い潰すのを防ぐ）。`ChangeExtractPort` の各メソッドは `&self` を取るため、呼び出しごとに更新する時刻は
/// `Cell` で内部可変にする。adapter は単一スレッドの record 経路でのみ使われるため `Cell` で十分（共有なし）。
pub(in crate::update_history) struct GithubModelsExtractAdapter {
    /// 直前のリクエスト開始時刻。未呼び出しの間は `None`（初回はペーシング待機なし）。
    last_request_at: Cell<Option<Instant>>,
    /// 抽出フェーズ開始時刻。未確定の間は `None`（最初の予算判定または抽出時に `Instant::now()` で確定する）。
    extract_phase_start: Cell<Option<Instant>>,
}

impl Default for GithubModelsExtractAdapter {
    fn default() -> Self {
        Self {
            last_request_at: Cell::new(None),
            extract_phase_start: Cell::new(None),
        }
    }
}

impl GithubModelsExtractAdapter {
    /// composition root から結線するための adapter を生成する（ペーシング状態は未初期化＝初回は即時）。
    pub(in crate::update_history) fn new() -> Self {
        Self::default()
    }

    /// 抽出フェーズ開始からの経過時間を返す（未確定なら今を起点に確定して 0 を返す）。
    ///
    /// 抽出フェーズの起点は「最初に予算判定または抽出を行った時点」とする。bump/eval は別 CI job、ノート取得
    /// や版差分算出はこの起点より前に走るため、起点は抽出ループ突入時刻に十分近い。`Cell` で内部可変に確定する
    /// （単一スレッドの record 経路専用）。
    fn elapsed_since_phase_start(&self) -> Duration {
        match self.extract_phase_start.get() {
            Some(start) => start.elapsed(),
            None => {
                self.extract_phase_start.set(Some(Instant::now()));
                Duration::ZERO
            }
        }
    }

    /// 抽出フェーズ開始から [`EXTRACT_BUDGET`] までの残予算を返す（超過済みなら `Duration::ZERO`）。
    fn remaining_budget(&self) -> Duration {
        EXTRACT_BUDGET.saturating_sub(self.elapsed_since_phase_start())
    }

    /// `GITHUB_TOKEN` を読む。未設定/空なら `None`（抽出を空へ縮退）。
    fn github_token() -> Option<String> {
        std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty())
    }

    /// エージェントループ初回の会話メッセージ列（system + user）を組み立てる。
    ///
    /// user メッセージはパッケージ名・old→new・ヒント URL 群（homepage/repo/changelog）と、機械解決で取れた
    /// seed ノート（あれば truncate 済み）を載せる。seed があっても AI は更に fetch_url で補える。seed ノートは
    /// 信頼境界外だが、後段で抽出結果を機械バリデートするため会話材料として与えてよい。
    fn initial_messages(request: &ExtractRequest) -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({ "role": "system", "content": EXTRACT_SYSTEM_PROMPT }),
            serde_json::json!({ "role": "user", "content": user_prompt(request) }),
        ]
    }

    /// tool-use ターンのリクエストボディ JSON を組み立てる（messages + tools + tool_choice=auto）。
    ///
    /// AI が `fetch_url` を自分で呼べるよう tools を与え、呼ぶか否かは AI に委ねる（`tool_choice: auto`）。
    /// response_format はこのターンでは付けない（ツール使用中に schema を強制すると tool_call を出せなくなる
    /// ため、最終要約ターン [`final_body`] でのみ schema を強制する）。
    fn tool_turn_body(messages: &[serde_json::Value]) -> Result<String> {
        let body = serde_json::json!({
            "model": EXTRACT_MODEL,
            "temperature": EXTRACT_TEMPERATURE,
            "messages": messages,
            "tools": [fetch_url_tool()],
            "tool_choice": "auto",
        });
        Ok(serde_json::to_string(&body)?)
    }

    /// 最終要約ターンのリクエストボディ JSON を組み立てる（messages + response_format 強制、tools 無し）。
    ///
    /// 十分なノートを読み終えた（または反復上限に達した）会話に対し、tools を外し strict schema の
    /// response_format を付けて構造化変更リストを返させる。tools を外すことで AI は要約へ収束し、schema 強制で
    /// 出力形を固定する。
    fn final_body(messages: &[serde_json::Value]) -> Result<String> {
        let body = serde_json::json!({
            "model": EXTRACT_MODEL,
            "temperature": EXTRACT_TEMPERATURE,
            "messages": messages,
            "response_format": response_format_schema(),
        });
        Ok(serde_json::to_string(&body)?)
    }

    /// curl で GitHub Models へ POST し、`(HTTP status, レスポンス本文, レスポンスヘッダ JSON)` を返す。
    /// curl 自体の失敗のみ `Err`。
    ///
    /// 認証トークンは **argv に乗せない**。`-H "Authorization: Bearer <token>"` を引数に置くと、同一 runner の
    /// プロセス一覧（`ps`）から token が読めてしまう（secret を argv/ログに残さない義務に違反する）。代わりに
    /// curl の `--config -`（stdin から設定を読む）へ `header = "Authorization: Bearer <token>"` を流し込み、
    /// token を argv にもログにも出さない。Content-Type ヘッダと本文（`-d`）は secret ではないため argv のままで
    /// よい。stdin の内容（[`auth_config`]）は curl 設定ファイル構文で、token をクォートして 1 ヘッダだけ渡す。
    ///
    /// 診断のため、HTTP エラー（4xx/5xx）でも curl を非 0 終了させない。`--fail` は HTTP エラーを curl exit へ
    /// 倒し本物の status code を握り潰す（CI ログに「なぜ空縮退したか」が残らない原因だった）ので使わない。
    /// 代わりに `--write-out` で本文末尾へ [`CURL_META_SENTINEL`] に続けて `http_code` と `header_json` を
    /// 付加し、[`split_meta`] で本文・status・ヘッダ JSON へ切り分ける。ヘッダ JSON は 429 の `Retry-After`
    /// を尊重するために取得する（[`post_with_retry`]）。`-w` を足しても Authorization は stdin の `--config -`
    /// に閉じたままで、argv・ログ・`-w` の出力いずれにも token は現れない（出力は status とレスポンスヘッダのみで、
    /// リクエストの Authorization ヘッダは `%{header_json}` の対象外）。返り値 `Err` は curl プロセス自体の失敗
    /// （spawn 失敗・ネットワーク不達等で非 0 終了）に限る。
    fn post(token: &str, body: &str) -> Result<(u16, String, String)> {
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
            // 本文末尾へ sentinel + status + レスポンスヘッダ JSON を付加する。token は含まれない
            // （%{http_code} は数値、%{header_json} は **レスポンス**ヘッダのみでリクエスト Authorization は出ない）。
            OsString::from("--write-out"),
            OsString::from(format!(
                "{CURL_META_SENTINEL}%{{http_code}}\n%{{header_json}}"
            )),
            OsString::from(GITHUB_MODELS_ENDPOINT),
        ];
        let raw = run_capture_with_stdin("curl", args, auth_config(token).as_bytes())?;
        Ok(split_meta(&raw))
    }

    /// [`post`] を呼び、HTTP 429（レート制限）なら待機して有界回数だけ再試行する。curl 自体の失敗のみ `Err`。
    ///
    /// 429 を受けたら [`retry_after_seconds`] で `Retry-After` ヘッダ（秒）を読み、あればその秒数、無ければ
    /// [`backoff_delay`] の指数バックオフ（[`BACKOFF_BASE`] × 2^n、[`MAX_BACKOFF`] 上限）だけ待ってから再試行する。
    /// 再試行は最大 [`MAX_RATE_LIMIT_RETRIES`] 回。それでも 429 が続けば最後の `(429, body, headers)` を返し、
    /// 呼び出し側が診断ログを出して空配列へ縮退する（daily 上限など待っても解消しない 429 で無限に粘らない）。
    /// 429 以外（200 や他のエラー status）は再試行せず即返す（401/403/413 等はバックオフで解消しないため）。
    /// 待機は同期 blocking 実行文脈（curl は blocking `std::process::Command`）に合わせ [`sleep`] で行う。
    /// 再試行ループ本体（status 判定・待機計算・有界回数・残予算判定）は network/sleep に依存しない純粋部分として
    /// [`retry_loop`] へ切り出し、ここでは実 [`post`]・実 [`sleep`]・実残予算（[`remaining_budget`](Self::remaining_budget)）
    /// を注入するだけにする（ループ規約は hermetic にテスト可能）。
    ///
    /// 残予算: 抽出フェーズの wall-clock 予算（[`EXTRACT_BUDGET`]）の残りを各待機の前に確認し、待機が deadline を
    /// 跨ぐ（残予算より長い）なら待たずに最後の 429 を返して縮退する（deadline を超える待機をしない）。
    fn post_with_retry(&self, token: &str, body: &str) -> Result<(u16, String, String)> {
        retry_loop(
            || Self::post(token, body),
            sleep,
            || self.remaining_budget(),
        )
    }

    /// 直前のリクエスト開始から [`MIN_REQUEST_INTERVAL`] が経つまで待機し、複数パッケージにまたがる呼び出し列へ
    /// per-minute レート上限内のペースを敷く。待機後に「今回の開始時刻」を記録する。
    ///
    /// 初回（`last_request_at` が `None`）は待機しない。`Cell` で内部可変に時刻を持つ（単一スレッドの record
    /// 経路専用）。待機は同期 blocking 文脈に合わせ [`sleep`] で行う。
    fn pace_before_request(&self) {
        if let Some(last) = self.last_request_at.get() {
            let elapsed = last.elapsed();
            if elapsed < MIN_REQUEST_INTERVAL {
                sleep(MIN_REQUEST_INTERVAL - elapsed);
            }
        }
        self.last_request_at.set(Some(Instant::now()));
    }

    /// 最終要約ターンのレスポンス本文（チャット補完 JSON）から変更項目列を取り出す。
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
        parse_change_items(&choice.message.content)
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

/// `fetch_url` ツールの JSON 定義（OpenAI 互換 function tool）。
///
/// AI は `fetch_url({"url": "..."})` でリリースノート/changelog を自分で取得できる。description で「許可された
/// ドメインの https URL のみ」「取得テキストはリリースノート要約の根拠」を AI に伝える。許可外 URL の実行拒否は
/// adapter 側の SSRF 検査（[`fetch_host_allowed`]）が機械的に担保し、prompt の指示だけに依存しない。
fn fetch_url_tool() -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": FETCH_TOOL_NAME,
            "description": "指定した https URL の本文を取得して返す。許可されたドメイン（パッケージの \
    homepage / リポジトリ / GitHub 公式）の https URL のみ取得でき、許可外は『not allowed』を返す。\
    リリースノートや changelog の取得に使う。",
            "parameters": {
                "type": "object",
                "additionalProperties": false,
                "required": ["url"],
                "properties": {
                    "url": { "type": "string", "description": "取得する https URL" }
                }
            }
        }
    })
}

/// エージェント初回 user メッセージの本文を組み立てる純粋関数。
///
/// パッケージ名・更新前後バージョン・ヒント URL（homepage/repo/changelog）を AI へ提示し、seed ノートがあれば
/// truncate して添える。AI はこれらを手がかりに fetch_url で適切なノートを自分で取得する。
fn user_prompt(request: &ExtractRequest) -> String {
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

/// 最終ターン content（`{ "changes": [...] }`）から [`ChangeItem`] 列を取り出す純粋関数。
///
/// `changes` は要素単位で try-parse し、未知 category や型不一致の item はその 1 件だけ drop する（一括
/// deserialize だと不正 1 件で全 changes が空へ縮退するため）。content の JSON 解析失敗は空配列。host/長さ/件数の
/// 機械バリデートは domain 側で別途行う。
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

/// チャット補完レスポンスから最初の choice の `(tool_calls, finish_reason)` を取り出す純粋関数。
///
/// エージェントループはこれで「AI が `fetch_url` を呼びたいか」を判定する。JSON 解析失敗・`choices` 不在は
/// 「tool_call 無し」（空 Vec, `None`）として扱い、ループは最終要約ターンへ移る。`finish_reason` は診断と
/// 「tool_calls を出したか」の補助判定に使う。
fn parse_turn(response: &str) -> (Vec<ToolCall>, Option<String>) {
    let completion: ChatCompletion = match serde_json::from_str(response) {
        Ok(value) => value,
        Err(_) => return (Vec::new(), None),
    };
    match completion.choices.into_iter().next() {
        Some(choice) => (choice.message.tool_calls, choice.finish_reason),
        None => (Vec::new(), None),
    }
}

/// tool_call の `arguments`（JSON 文字列 `{"url": "..."}`）から URL を取り出す純粋関数。
///
/// `name` が [`FETCH_TOOL_NAME`] でない・arguments が JSON でない・`url` フィールドが無い/非文字列は `None`
/// （その tool_call は無視する）。AI が渡した URL の host 許可判定は呼び出し側（fetch 実行前）が行う。
fn tool_call_url(call: &ToolCall) -> Option<String> {
    if call.function.name != FETCH_TOOL_NAME {
        return None;
    }
    let args: serde_json::Value = serde_json::from_str(&call.function.arguments).ok()?;
    args.get("url")?.as_str().map(str::to_string)
}

/// AI が tool_call で要求した assistant メッセージを会話へ追加する形（OpenAI 互換）に組み立てる。
///
/// 再 request では、AI の tool_call を含む assistant メッセージと、それに対応する `role: tool` 応答を順に
/// 会話へ積む必要がある。assistant メッセージは `tool_calls` を `id`/`function.name`/`function.arguments` で
/// echo back する（API がツール応答を紐付けるため）。content は空文字にする。
fn assistant_tool_call_message(calls: &[ToolCall]) -> serde_json::Value {
    let tool_calls: Vec<serde_json::Value> = calls
        .iter()
        .map(|call| {
            serde_json::json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.function.name,
                    "arguments": call.function.arguments,
                }
            })
        })
        .collect();
    serde_json::json!({
        "role": "assistant",
        "content": "",
        "tool_calls": tool_calls,
    })
}

/// tool_call への `role: tool` 応答メッセージを組み立てる（`tool_call_id` で紐付け）。
fn tool_result_message(tool_call_id: &str, content: &str) -> serde_json::Value {
    serde_json::json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content,
    })
}

/// tool-use エージェントループ本体（network/fetch/sleep に依存しない純粋規約）。
///
/// `request` は 1 model 呼び出し（body JSON を渡し、レスポンス本文文字列を返す）。`fetch` は SSRF 検査つきの
/// URL 取得（許可外/失敗は `None`）。両 closure を注入することで、実 GitHub Models・実 curl を呼ばずにループ
/// 規約（tool_call→fetch→再 request→最終 schema・有界反復）を hermetic にテストできる。
///
/// 進行:
/// 1. 初回 [`tool_turn_body`]（messages + tools + tool_choice=auto）で `request`。
/// 2. レスポンスの tool_calls を [`parse_turn`] で読む。tool_calls があり反復が [`MAX_TOOL_ITERATIONS`] 未満なら、
///    各 `fetch_url(url)` を `fetch` で実行（1 ターン最大 [`MAX_TOOL_CALLS_PER_TURN`] 件、許可外/失敗は「not
///    allowed」/「fetch failed」をツール結果に）、assistant の tool_call と `role: tool` 応答を会話へ積んで再
///    [`tool_turn_body`] で `request`。
/// 3. tool_calls が無くなる or 反復上限に達したら、tools を外し response_format を付けた [`final_body`] で
///    `request` し、[`parse_change_items`] で構造化変更を取り出す。
/// 4. `request` の `Err`（curl プロセス失敗）はそのまま伝播（呼び出し側が縮退）。
///
/// SSRF 許可ホスト判定は注入された `fetch` closure 内に閉じる（[`fetch_allowed_note`] が host 集合で機械判定し、
/// 許可外は `None`）。ループ本体は host 集合を直接持たず、「`fetch` が `None` を返したら『not allowed』を AI へ
/// 返す」という規約だけを担う（host 集合の組み立ては呼び出し側の責務）。
fn agent_loop<R, F>(
    request: &ExtractRequest,
    mut model_request: R,
    mut fetch: F,
) -> Result<ExtractOutcome>
where
    R: FnMut(&str) -> Result<String>,
    F: FnMut(&str) -> Result<Option<String>>,
{
    let mut messages = GithubModelsExtractAdapter::initial_messages(request);
    // AI が会話中に最後に成功 fetch（非 None）した URL を採用取得元として保持する。これを provenance として
    // `origin=ai-discovered` でレジストリへ学習・再利用する経路にする（利用者要件 (3)/(4)）。許可外/取得失敗
    // （None）では更新しない。最後の成功 fetch を採るのは、AI が複数 fetch を試した末に有効ノートへ収束した
    // URL を採用取得元と見なすためである（採用ノートに最も近い）。
    let mut adopted_source: Option<String> = None;
    for _ in 0..MAX_TOOL_ITERATIONS {
        let body = GithubModelsExtractAdapter::tool_turn_body(&messages)?;
        let response = model_request(&body)?;
        let (calls, _finish_reason) = parse_turn(&response);
        let fetch_calls: Vec<&ToolCall> = calls
            .iter()
            .filter(|call| call.function.name == FETCH_TOOL_NAME)
            .collect();
        if fetch_calls.is_empty() {
            // AI はもうツールを呼ばない。最終要約ターンへ移る。
            break;
        }
        // AI の tool_call を assistant メッセージとして echo back し、各呼び出しへ tool 応答を積む。
        messages.push(assistant_tool_call_message(&calls));
        for call in calls.iter().take(MAX_TOOL_CALLS_PER_TURN) {
            // fetch_url 以外の tool_call は紐付けのため空応答を返す（API は全 tool_call へ応答を要求する）。
            let result = match tool_call_url(call) {
                Some(url) => match fetch(&url)? {
                    Some(text) => {
                        // 成功 fetch（SSRF 検査通過 + 非空本文）の URL を採用取得元候補として記録する。
                        adopted_source = Some(url);
                        truncate_notes(&text)
                    }
                    // 許可外 host または取得失敗。AI は別 URL を試せる（採用取得元は更新しない）。
                    None => String::from("not allowed or fetch failed"),
                },
                None => String::from("unsupported tool"),
            };
            messages.push(tool_result_message(&call.id, &result));
        }
        // MAX_TOOL_CALLS_PER_TURN を超えた残り tool_call にも紐付け応答を積む（API 整合）。
        for call in calls.iter().skip(MAX_TOOL_CALLS_PER_TURN) {
            messages.push(tool_result_message(&call.id, "skipped (too many calls)"));
        }
    }
    // 最終要約ターン: tools を外し response_format 強制で構造化変更を返させる。
    let final_body = GithubModelsExtractAdapter::final_body(&messages)?;
    let response = model_request(&final_body)?;
    Ok(ExtractOutcome {
        items: GithubModelsExtractAdapter::parse_response(&response),
        source_url: adopted_source,
    })
}

/// seed ノートが「十分にある」（要約の根拠に足る非空テキストを持つ）かを判定する純粋関数。
///
/// `extract_change_items` の経路分岐に使う: seed が十分にあれば（registry 再利用 fetch 成功 or 機械解決成功）
/// ツール探索せず seed の **要約のみ 1 回**（[`summarize_only`]）、無ければ AI に fetch_url で探索させる
/// （[`agent_loop`]）。判定は「`seed_notes` が `Some` でかつ本文が trim 後非空」とする。空白だけの seed は
/// 根拠にならないため「無し」扱いにして探索経路へ倒す。
fn has_sufficient_seed(request: &ExtractRequest) -> bool {
    request
        .seed_notes
        .as_ref()
        .is_some_and(|notes| !notes.text.trim().is_empty())
}

/// seed ノートを根拠に **ツールを与えず 1 回だけ** structured output（strict schema）で要約する純粋規約。
///
/// registry 再利用 fetch 成功・機械解決成功で seed ノートが既に取得済みのとき（[`has_sufficient_seed`]）に使う。
/// `fetch_url` ツールを提示しないため AI は探索できず、**GitHub Models 呼び出しは 1 回**で終わる（registry が
/// 埋まるほど探索が新規/未知のみへ収束し、レート消費が回を追って実際に逓減する核）。会話は初回 user メッセージ
/// （seed を含む）に対し即 [`final_body`]（tools 無し・response_format 強制）を投げ、[`parse_change_items`] で
/// 構造化変更を取り出す。
///
/// ハルシネーション防止: 根拠は与えた seed の実ノートのみ（[`EXTRACT_SYSTEM_PROMPT`] の「与えたノートのみを
/// 根拠とし、無ければ空」契約は tools の有無に依らず維持される）。`source_url` は **常に `None`** を返す
/// （ai-discovered 学習は探索経路でのみ更新し、summarize_only 経路では既存 source を据え置く＝reused は origin
/// 維持、mechanical は自身の取得元 URL を別途学習済み）。`request` の `Err`（curl プロセス失敗）はそのまま伝播。
fn summarize_only<R>(request: &ExtractRequest, mut model_request: R) -> Result<ExtractOutcome>
where
    R: FnMut(&str) -> Result<String>,
{
    let messages = GithubModelsExtractAdapter::initial_messages(request);
    let final_body = GithubModelsExtractAdapter::final_body(&messages)?;
    let response = model_request(&final_body)?;
    Ok(ExtractOutcome {
        items: GithubModelsExtractAdapter::parse_response(&response),
        // summarize_only 経路では採用取得元を学習しない（ai-discovered は探索経路でのみ更新）。
        source_url: None,
    })
}

/// curl の stdout（本文）+ `--write-out` トレーラ（[`CURL_META_SENTINEL`] + status + ヘッダ JSON）出力から
/// `(status, body, header_json)` を切り出す。
///
/// curl は本文をそのまま stdout へ流し、その後ろへ sentinel・`%{http_code}`（3 桁数値）・`%{header_json}`
/// （レスポンスヘッダの JSON オブジェクト）を改行区切りで追記する。sentinel は本文と衝突しない一意文字列なので、
/// **最後の** sentinel 以降をトレーラとして切り出す（本文へ sentinel と同一文字列が現れる確率は無視できるが、
/// 万一現れても最後の出現＝curl が付けたトレーラを採る）。sentinel が見つからない想定外出力は status `0`・
/// ヘッダ空とし、呼び出し側が HTTP エラー扱いで診断ログを出す（縮退は維持）。token はこの出力に現れないため、
/// ログへ本文断片を出しても secret は漏れない（status は数値、header_json は**レスポンス**ヘッダのみ）。
fn split_meta(raw: &str) -> (u16, String, String) {
    let Some(sentinel_at) = raw.rfind(CURL_META_SENTINEL) else {
        return (0, raw.to_string(), String::new());
    };
    let body = raw[..sentinel_at].to_string();
    let trailer = &raw[sentinel_at + CURL_META_SENTINEL.len()..];
    // トレーラは "<http_code>\n<header_json>"。最初の改行で status とヘッダ JSON を分ける。
    let (status_text, header_json) = match trailer.split_once('\n') {
        Some((status_text, header_json)) => (status_text, header_json),
        None => (trailer, ""),
    };
    let status = status_text.trim().parse::<u16>().unwrap_or(0);
    (status, body, header_json.to_string())
}

/// curl の `%{header_json}` 出力（レスポンスヘッダの JSON）から `Retry-After`（秒）を読む。
///
/// `%{header_json}` はヘッダ名を小文字キー・値を文字列配列にした JSON オブジェクトを出す（例:
/// `{"retry-after":["12"],...}`）。`Retry-After` の秒数表現のみを尊重し、HTTP-date 形式や非数値・欠落は
/// `None`（呼び出し側は指数バックオフへ fallback）とする。解析できない JSON も `None`。
fn retry_after_seconds(header_json: &str) -> Option<u64> {
    let headers: serde_json::Value = serde_json::from_str(header_json).ok()?;
    let value = headers.get(RETRY_AFTER_HEADER)?;
    // header_json は値を配列で持つ（同名ヘッダ複数対応）。先頭要素を秒数として読む。
    let raw = value
        .as_array()
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.as_str())?;
    raw.trim().parse::<u64>().ok()
}

/// 指数バックオフの待機時間を求める（`Retry-After` が無い 429 の fallback）。
///
/// retry 回数 `attempt`（0 始まり）に対し `BACKOFF_BASE × 2^attempt` を返し、[`MAX_BACKOFF`] で頭打ちにする。
/// `2^attempt` は overflow しない範囲（`MAX_RATE_LIMIT_RETRIES` は小さい）で計算し、上限 clamp で異常値を防ぐ。
fn backoff_delay(attempt: u32) -> Duration {
    let multiplier = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
    BACKOFF_BASE
        .checked_mul(multiplier)
        .unwrap_or(MAX_BACKOFF)
        .min(MAX_BACKOFF)
}

/// HTTP 429 のバックオフ付き再試行ループ本体（network/sleep に依存しない純粋規約）。
///
/// `request` を呼んで `(status, body, headers)` を得る。429 以外（200 や 401/403/413 等）はバックオフで解消
/// しないため再試行せず即返す。429 なら待機時間を [`Retry-After`](RETRY_AFTER_HEADER)（あれば [`MAX_BACKOFF`]
/// で clamp）または [`backoff_delay`] の指数バックオフで決め、`wait` へ渡して待たせてから再試行する。再試行は
/// 最大 [`MAX_RATE_LIMIT_RETRIES`] 回で、それでも 429 が続けば最後の `(429, body, headers)` を返す（呼び出し側
/// が空配列へ縮退＝daily 上限など待っても解消しない 429 で無限に粘らない）。`request` の `Err` はそのまま伝播する
/// （curl プロセス自体の失敗）。`wait` を closure で受けることで、テストは実 sleep せず待機時間を観測できる。
///
/// `remaining_budget` は抽出フェーズ全体の wall-clock 残予算（[`EXTRACT_BUDGET`]）を返す。待機を入れる前に残予算を
/// 確認し、**待機が残予算を超える（deadline を跨ぐ）なら待たずに**最後の 429 を返して縮退する（deadline を超える
/// 待機をしない）。これにより 1 リクエストのリトライ待機が抽出フェーズ予算を食い破らない。`remaining_budget` も
/// closure で受け、テストは実時計に依存せず残予算を注入できる。
fn retry_loop<R, W, B>(
    mut request: R,
    mut wait: W,
    mut remaining_budget: B,
) -> Result<(u16, String, String)>
where
    R: FnMut() -> Result<(u16, String, String)>,
    W: FnMut(Duration),
    B: FnMut() -> Duration,
{
    let mut attempt = 0;
    loop {
        let (status, response, headers) = request()?;
        if status != 429 || attempt >= MAX_RATE_LIMIT_RETRIES {
            return Ok((status, response, headers));
        }
        let delay = retry_after_seconds(&headers)
            .map(Duration::from_secs)
            .map(|d| d.min(MAX_BACKOFF))
            .unwrap_or_else(|| backoff_delay(attempt));
        // 待機が抽出フェーズ予算（deadline）を跨ぐなら待たずに縮退する（deadline を超える待機をしない）。
        // 残予算 0（既に超過）でも待たず、進行中だった本リクエストの 429 をそのまま返して version-only へ倒す。
        if delay > remaining_budget() {
            eprintln!(
                "GitHub Models extract: 429 retry wait {}s would exceed extract budget, degrading to version-only",
                delay.as_secs()
            );
            return Ok((status, response, headers));
        }
        // 429 は CI ログで可視化する（なぜ record が遅延したかの根拠）。token は含まれない。
        eprintln!(
            "GitHub Models extract rate-limited: HTTP 429, retry {}/{MAX_RATE_LIMIT_RETRIES} after {}s",
            attempt + 1,
            delay.as_secs()
        );
        wait(delay);
        attempt += 1;
    }
}

/// レート制限待機・ペーシングの blocking 待機。
///
/// record 経路は `current_thread` tokio runtime 内で実行されるが、HTTP は blocking な外部 `curl`
/// （`std::process::Command`）で同期に行うため、待機もこの同期文脈に合わせて [`std::thread::sleep`] で行う。
/// record 経路には同時に走る他 async task が無いため、この thread を待たせても他作業を阻害しない（nested
/// `block_on` は current_thread runtime で panic するため使わない）。`Duration::ZERO` 以下では即返る。
fn sleep(duration: Duration) {
    if duration.is_zero() {
        return;
    }
    std::thread::sleep(duration);
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

impl GithubModelsExtractAdapter {
    /// 1 model 呼び出しを実行する: ペーシングを敷き、429 リトライつきで POST し、200 本文を返す。
    ///
    /// 非 200（429・他 HTTP エラー）と curl 失敗は `Err` にして agent_loop の外（[`extract_change_items`]）へ
    /// 伝播させ、そこで空配列へ縮退させる。診断（429/他 HTTP/curl 失敗の区別）はここで 1 行ログする（token は
    /// 出さない）。各 model 呼び出しの前に [`pace_before_request`] で per-minute レート上限内のペースを敷く
    /// （エージェントは 1 パッケージで複数 model 呼び出しをするため、ペーシング/予算は model 呼び出し単位で効かせる）。
    fn model_request(&self, token: &str, body: &str) -> Result<String> {
        self.pace_before_request();
        match self.post_with_retry(token, body) {
            Ok((200, response, _headers)) => Ok(response),
            Ok((429, _response, _headers)) => {
                eprintln!(
                    "GitHub Models extract degraded: HTTP 429 after {MAX_RATE_LIMIT_RETRIES} bounded retries (rate-limited)"
                );
                anyhow::bail!("github models rate-limited (429)")
            }
            Ok((status, response, _headers)) => {
                eprintln!(
                    "GitHub Models extract degraded: HTTP {status}: {}",
                    body_snippet(&response)
                );
                anyhow::bail!("github models HTTP {status}")
            }
            Err(error) => {
                eprintln!("GitHub Models extract degraded: curl failed");
                Err(error)
            }
        }
    }
}

impl ChangeExtractPort for GithubModelsExtractAdapter {
    /// ノートを要約して構造化変更へ抽出する。seed の有無で **要約のみ 1 回**と **探索（有界）**を切り替える。
    /// 失敗は record を止めず空配列へ縮退する。
    ///
    /// **経路分岐（レート逓減の核）**:
    /// - **seed が十分にある場合**（[`has_sufficient_seed`]＝registry 再利用 fetch 成功 or 機械解決成功で seed
    ///   ノートが既に取得済み）: **ツールを与えず seed を根拠に 1 回だけ** structured output で要約する
    ///   （[`summarize_only`]）。`fetch_url` 探索をしないため **GitHub Models 呼び出しは 1 回**。registry が
    ///   埋まるほど機械的に取れる/学習済みのものが 1 回化され、GitHub Models のレート消費が回を追って実際に逓減する
    ///   （利用者要件 4 が真になる）。ai-discovered 学習は行わない（`source_url=None`、既存 source 据え置き）。
    /// - **seed が無い/空の場合のみ**: 既存の [`agent_loop`]（`fetch_url` tool-use 探索、最大
    ///   [`MAX_TOOL_ITERATIONS`]+1 回 model 呼び出し）で AI にノートを探させる。AI が採用した取得元 URL を
    ///   `source_url` で返し、`origin=ai-discovered` 学習経路へ渡す。
    ///
    /// ハルシネーション防止は両経路で維持する（与えた seed の実ノートのみを根拠とし、無ければ空。
    /// [`EXTRACT_SYSTEM_PROMPT`] 契約は tools の有無に依らない）。
    ///
    /// SSRF: 探索経路の fetch 許可ホスト集合は **eval メタ由来のヒント**（repo/homepage/changelog）だけから
    /// [`allowed_fetch_hosts`] で組み立て、ノート本文（信頼境界外）では拡張しない。AI が要求した URL の host が
    /// 集合外なら [`fetch_allowed_note`] が fetch せず `None` を返し、ループは「not allowed」を AI へ返す。
    /// summarize_only 経路は fetch せず seed のみを根拠にするため、許可ホスト集合は組み立てない。
    ///
    /// 縮退契約（[module 先頭][self] 参照）は維持しつつ、**なぜ空になったか**を非致命の 1 行診断として stderr へ
    /// 出す（token 未設定（`skipped`）、curl プロセス失敗、HTTP 非 200）。token はいずれのログにも出さない。
    /// 返り値は常に `Ok`（解析結果または空配列）で record を止めない。
    ///
    /// レート制限/予算: 各 model 呼び出しの前に [`pace_before_request`](Self::pace_before_request) で
    /// [`MIN_REQUEST_INTERVAL`] のペースを敷き、HTTP 呼び出しは [`post_with_retry`](Self::post_with_retry) で
    /// 429 のバックオフ付き再試行を行う。有界回数でも 429 が解消しない場合は空配列へ縮退する（record success
    /// 維持）。ペーシング/予算は **model 呼び出し単位**で効く（探索経路は 1 パッケージで複数回呼ぶ）。
    fn extract_change_items(&self, request: &ExtractRequest) -> Result<ExtractOutcome> {
        // GITHUB_TOKEN 未設定なら呼び出さず空へ縮退（version+notes_url へフォールバック。採用取得元も無し）。
        let Some(token) = Self::github_token() else {
            eprintln!("GitHub Models extract skipped: GITHUB_TOKEN unset");
            return Ok(ExtractOutcome::default());
        };
        // seed が十分にあれば探索せず要約のみ 1 回（レート逓減）。無い/空のときだけ AI に fetch_url 探索させる。
        let result = if has_sufficient_seed(request) {
            // summarize_only: tools を与えず seed を根拠に 1 回だけ要約する（GitHub Models 呼び出しは 1 回）。
            summarize_only(request, |body| self.model_request(&token, body))
        } else {
            // fetch 許可ホスト集合は eval メタ由来のヒントだけから組み立てる（ノート本文では拡張しない＝SSRF 防御）。
            let allowed_hosts = allowed_fetch_hosts(
                request.repo.as_deref(),
                request.homepage.as_deref(),
                request.changelog.as_deref(),
            );
            // model 呼び出しは self.model_request（ペーシング + 429 リトライ + 診断 + 非 200/curl 失敗を Err 化）、
            // fetch は SSRF 検査つき fetch_allowed_note を注入する。
            agent_loop(
                request,
                |body| self.model_request(&token, body),
                |url| fetch_allowed_note(url, &allowed_hosts),
            )
        };
        match result {
            Ok(outcome) => Ok(outcome),
            // model 呼び出しの失敗（レート枯渇・HTTP エラー・curl 失敗）は record を止めず空配列へ縮退する。
            // 診断は model_request 側で 1 行ログ済み。
            Err(_) => Ok(ExtractOutcome::default()),
        }
    }

    /// 抽出フェーズの wall-clock 予算（[`EXTRACT_BUDGET`]）を使い切ったかを返す。
    ///
    /// 抽出フェーズ開始（[`elapsed_since_phase_start`](Self::elapsed_since_phase_start) で初回に確定）からの
    /// 経過が予算以上なら `true`。外部 I/O はせず、内部の経過時間だけで判定する。caller（application）は各
    /// パッケージ抽出の前にこれを問い合わせ、`true` の間は LLM 抽出を呼ばず version-only へ縮退させる。
    fn extract_budget_exhausted(&self) -> bool {
        self.remaining_budget().is_zero()
    }
}

#[cfg(test)]
mod tests {
    //! チャット補完レスポンスからの変更項目抽出（category enum 検証・未知値破棄・空縮退）と
    //! リクエストボディ/スキーマ組み立て、および 429 レート制限のバックオフ計算・`Retry-After` 解釈・
    //! curl トレーラ（status/header JSON）切り出しという純粋部分を、実 API/network/sleep を呼ばずに固定する。

    use super::{
        BACKOFF_BASE, EXTRACT_BUDGET, GithubModelsExtractAdapter, MAX_BACKOFF, MAX_NOTES_CHARS,
        MAX_RATE_LIMIT_RETRIES, MAX_TOOL_CALLS_PER_TURN, MAX_TOOL_ITERATIONS, MIN_REQUEST_INTERVAL,
        TRUNCATION_MARKER, agent_loop, auth_config, backoff_delay, body_snippet,
        has_sufficient_seed, response_format_schema, retry_after_seconds, retry_loop, split_meta,
        summarize_only, tool_call_url, truncate_notes, user_prompt,
    };
    use crate::update_history::domain::validate::allowed_fetch_hosts;
    use crate::update_history::domain::wire::ChangeCategory;
    use crate::update_history::ports::{ChangeExtractPort, ExtractRequest, RawReleaseNotes};
    use std::cell::Cell;
    use std::time::Duration;

    /// テスト用 [`ExtractRequest`] を作る（name + old→new + repo ヒント + 任意 seed ノート）。
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
                notes_url: format!("https://github.com/{}/releases", repo.unwrap_or("o/r")),
            }),
        }
    }

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
    fn tool_turn_body_offers_fetch_tool_and_auto_choice() -> crate::Result<()> {
        // tool-use ターンは fetch_url ツールを与え tool_choice=auto で AI に呼ぶか委ねる。response_format は
        // このターンでは付けない（ツール使用を妨げないため）。
        let request = request_with("neovim", Some("neovim/neovim"), None);
        let messages = GithubModelsExtractAdapter::initial_messages(&request);
        let body = GithubModelsExtractAdapter::tool_turn_body(&messages)?;
        let value: serde_json::Value = serde_json::from_str(&body)?;
        assert_eq!(value["temperature"], 0.0);
        assert_eq!(value["tool_choice"], "auto");
        assert_eq!(value["tools"][0]["function"]["name"], "fetch_url");
        assert!(
            value["response_format"].is_null(),
            "tool ターンに schema は付けない"
        );
        // system + user の 2 メッセージで始まる。
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][1]["role"], "user");
        Ok(())
    }

    #[test]
    fn final_body_forces_schema_without_tools() -> crate::Result<()> {
        // 最終要約ターンは tools を外し response_format（strict schema）を強制する。
        let request = request_with("neovim", Some("neovim/neovim"), None);
        let messages = GithubModelsExtractAdapter::initial_messages(&request);
        let body = GithubModelsExtractAdapter::final_body(&messages)?;
        let value: serde_json::Value = serde_json::from_str(&body)?;
        assert_eq!(value["temperature"], 0.0);
        assert_eq!(value["response_format"]["type"], "json_schema");
        assert!(value["tools"].is_null(), "最終ターンに tools は付けない");
        Ok(())
    }

    #[test]
    fn user_prompt_includes_name_versions_and_hint_urls() {
        // user prompt はパッケージ名・old→new・ヒント URL を AI へ提示する（fetch の手がかり）。
        let request = ExtractRequest {
            name: "neovim".to_string(),
            old: Some("0.10".to_string()),
            new: Some("0.11".to_string()),
            repo: Some("neovim/neovim".to_string()),
            homepage: Some("https://neovim.io/".to_string()),
            changelog: Some(
                "https://github.com/neovim/neovim/blob/master/CHANGELOG.md".to_string(),
            ),
            seed_notes: None,
        };
        let prompt = user_prompt(&request);
        assert!(prompt.contains("neovim"));
        assert!(prompt.contains("0.10"));
        assert!(prompt.contains("0.11"));
        assert!(prompt.contains("https://github.com/neovim/neovim/releases"));
        assert!(prompt.contains("https://neovim.io/"));
    }

    #[test]
    fn tool_call_url_reads_url_from_fetch_arguments() {
        // fetch_url tool_call の arguments(JSON) から url を取り出す。別ツール名・不正 JSON は None。
        let call = super::ToolCall {
            id: "c1".to_string(),
            function: super::ToolCallFunction {
                name: "fetch_url".to_string(),
                arguments: r#"{"url":"https://github.com/o/r/releases"}"#.to_string(),
            },
        };
        assert_eq!(
            tool_call_url(&call).as_deref(),
            Some("https://github.com/o/r/releases")
        );
        let other = super::ToolCall {
            id: "c2".to_string(),
            function: super::ToolCallFunction {
                name: "other_tool".to_string(),
                arguments: r#"{"url":"https://github.com/x"}"#.to_string(),
            },
        };
        assert_eq!(tool_call_url(&other), None);
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

    /// curl の `--write-out` トレーラ（sentinel + status + header JSON）を本文末尾へ付けた raw 文字列を組む
    /// test helper。実 curl/network を呼ばずに [`split_meta`] の切り出しを固定するため。
    fn raw_with_meta(body: &str, status: u16, header_json: &str) -> String {
        format!("{body}{}{status}\n{header_json}", super::CURL_META_SENTINEL)
    }

    #[test]
    fn split_meta_separates_body_status_and_headers() {
        // curl は本文の末尾へ sentinel + `%{http_code}` + `%{header_json}` を付加する。sentinel 以降を
        // トレーラとして切り出し、本文・status・ヘッダ JSON へ分ける。
        let raw = raw_with_meta(r#"{"choices":[]}"#, 200, r#"{"retry-after":["5"]}"#);
        let (status, body, headers) = split_meta(&raw);
        assert_eq!(status, 200);
        assert_eq!(body, r#"{"choices":[]}"#);
        assert_eq!(headers, r#"{"retry-after":["5"]}"#);

        // HTTP エラー status も同様に切り出せる。
        let raw = raw_with_meta(r#"{"error":"forbidden"}"#, 403, "{}");
        let (status, body, _headers) = split_meta(&raw);
        assert_eq!(status, 403);
        assert_eq!(body, r#"{"error":"forbidden"}"#);
    }

    #[test]
    fn split_meta_returns_zero_when_no_sentinel() {
        // 想定外出力（sentinel 無し）は status 0・ヘッダ空（呼び出し側で HTTP エラー扱い→診断ログ→空縮退）。
        let (status, body, headers) = split_meta("not json no meta");
        assert_eq!(status, 0);
        assert_eq!(body, "not json no meta");
        assert!(headers.is_empty());
    }

    #[test]
    fn retry_after_seconds_reads_numeric_seconds_from_header_json() {
        // `%{header_json}` は値を文字列配列で持つ（小文字キー）。Retry-After の秒数表現を読む。
        assert_eq!(
            retry_after_seconds(r#"{"retry-after":["12"],"content-type":["application/json"]}"#),
            Some(12)
        );
    }

    #[test]
    fn retry_after_seconds_is_none_for_missing_or_nonnumeric() {
        // ヘッダ欠落・非数値（HTTP-date 等）・解析不能 JSON は None（指数バックオフへ fallback）。
        assert_eq!(
            retry_after_seconds(r#"{"content-type":["application/json"]}"#),
            None
        );
        assert_eq!(
            retry_after_seconds(r#"{"retry-after":["Wed, 21 Oct 2025 07:28:00 GMT"]}"#),
            None
        );
        assert_eq!(retry_after_seconds("not json"), None);
        assert_eq!(retry_after_seconds(""), None);
    }

    #[test]
    fn backoff_delay_is_exponential_and_capped() {
        // 退行固定: Retry-After 無しの 429 fallback は BACKOFF_BASE × 2^attempt（2s,4s,8s,16s）で、
        // MAX_BACKOFF（20s）で頭打ちにする。指数が上限を超えても 1 回の待機が読めなくならない。
        assert_eq!(backoff_delay(0), BACKOFF_BASE);
        assert_eq!(backoff_delay(1), BACKOFF_BASE * 2);
        assert_eq!(backoff_delay(2), BACKOFF_BASE * 4);
        assert_eq!(backoff_delay(3), BACKOFF_BASE * 8);
        // 大きな attempt でも上限 clamp（overflow せず MAX_BACKOFF を返す）。
        assert_eq!(backoff_delay(60), MAX_BACKOFF);
        // どの attempt でも 1 回の待機は MAX_BACKOFF を超えない。
        assert!(backoff_delay(4) <= MAX_BACKOFF);
    }

    /// retry_loop へ注入する「予算無制限」残予算 closure（deadline を跨ぐ待機抑止が無効＝従来挙動）。
    /// 待機時間がこの値を超えることはないため、deadline による縮退は発生しない。
    fn budget_unlimited() -> Duration {
        Duration::from_secs(u64::MAX)
    }

    /// retry_loop へ注入する request stub: 呼び出しごとに与えた `(status, body, headers)` を順に返し、
    /// 列を超えたら最後の要素を返し続ける。呼び出し回数も観測する。実 curl/network を呼ばない。
    struct RequestStub<'a> {
        responses: &'a [(u16, &'a str, &'a str)],
        calls: Cell<usize>,
    }

    impl<'a> RequestStub<'a> {
        fn new(responses: &'a [(u16, &'a str, &'a str)]) -> Self {
            Self {
                responses,
                calls: Cell::new(0),
            }
        }

        fn next(&self) -> crate::Result<(u16, String, String)> {
            let index = self.calls.get().min(self.responses.len() - 1);
            self.calls.set(self.calls.get() + 1);
            let (status, body, headers) = self.responses[index];
            Ok((status, body.to_string(), headers.to_string()))
        }
    }

    #[test]
    fn retry_loop_returns_immediately_on_success_without_waiting() -> crate::Result<()> {
        // 200 は再試行せず即返し、待機もしない。
        let stub = RequestStub::new(&[(200, r#"{"choices":[]}"#, "{}")]);
        let waits = Cell::new(0usize);
        let (status, body, _headers) = retry_loop(
            || stub.next(),
            |_| waits.set(waits.get() + 1),
            budget_unlimited,
        )?;
        assert_eq!(status, 200);
        assert_eq!(body, r#"{"choices":[]}"#);
        assert_eq!(stub.calls.get(), 1);
        assert_eq!(waits.get(), 0, "成功時は待機しない");
        Ok(())
    }

    #[test]
    fn retry_loop_retries_on_429_then_succeeds() -> crate::Result<()> {
        // 退行固定: 429 を受けたら待機して再試行し、後続の 200 で成功する。
        let stub = RequestStub::new(&[
            (429, "rate limited", "{}"),
            (429, "rate limited", "{}"),
            (200, r#"{"choices":[]}"#, "{}"),
        ]);
        let waits = Cell::new(0usize);
        let (status, _body, _headers) = retry_loop(
            || stub.next(),
            |_| waits.set(waits.get() + 1),
            budget_unlimited,
        )?;
        assert_eq!(status, 200);
        assert_eq!(stub.calls.get(), 3, "429×2 の後 200 で成功＝3 回呼ぶ");
        assert_eq!(waits.get(), 2, "429 を受けた 2 回だけ待機する");
        Ok(())
    }

    #[test]
    fn retry_loop_degrades_after_bounded_retries_on_persistent_429() -> crate::Result<()> {
        // 退行固定: 429 が解消しない（daily 上限等）場合は MAX_RATE_LIMIT_RETRIES 回再試行した後、最後の
        // 429 を返す（呼び出し側が空配列へ縮退）。無限に粘らない・サイレントに無限待機しない。
        let stub = RequestStub::new(&[(429, "rate limited", "{}")]);
        let waits = Cell::new(0usize);
        let (status, _body, _headers) = retry_loop(
            || stub.next(),
            |_| waits.set(waits.get() + 1),
            budget_unlimited,
        )?;
        assert_eq!(status, 429, "解消しなければ最後の 429 を返す");
        // 初回 + 再試行回数 = 呼び出し回数。待機は再試行回数分だけ。
        let expected_calls = (MAX_RATE_LIMIT_RETRIES + 1) as usize;
        assert_eq!(stub.calls.get(), expected_calls);
        assert_eq!(waits.get(), MAX_RATE_LIMIT_RETRIES as usize);
        Ok(())
    }

    #[test]
    fn retry_loop_honors_retry_after_header_over_backoff() -> crate::Result<()> {
        // 退行固定: 429 の Retry-After（秒）があればその秒数を待つ（指数バックオフより優先）。
        let stub = RequestStub::new(&[
            (429, "rate limited", r#"{"retry-after":["7"]}"#),
            (200, r#"{"choices":[]}"#, "{}"),
        ]);
        let observed: Cell<Option<Duration>> = Cell::new(None);
        let (status, _body, _headers) =
            retry_loop(|| stub.next(), |d| observed.set(Some(d)), budget_unlimited)?;
        assert_eq!(status, 200);
        assert_eq!(
            observed.get(),
            Some(Duration::from_secs(7)),
            "Retry-After 7s を尊重する（指数バックオフ 2s ではない）"
        );
        Ok(())
    }

    #[test]
    fn retry_loop_does_not_retry_non_429_errors() -> crate::Result<()> {
        // 退行固定: 403/413 等はバックオフで解消しないため再試行せず即返す（無駄な待機をしない）。
        let stub = RequestStub::new(&[(403, "forbidden", "{}")]);
        let waits = Cell::new(0usize);
        let (status, _body, _headers) = retry_loop(
            || stub.next(),
            |_| waits.set(waits.get() + 1),
            budget_unlimited,
        )?;
        assert_eq!(status, 403);
        assert_eq!(stub.calls.get(), 1);
        assert_eq!(waits.get(), 0);
        Ok(())
    }

    #[test]
    fn retry_loop_does_not_wait_when_delay_exceeds_remaining_budget() -> crate::Result<()> {
        // 退行固定（deadline を跨ぐ待機の抑止）: 429 のリトライ待機が抽出フェーズの残予算を超える場合、待たずに
        // 最後の 429 を返して縮退する。残予算を待機時間より小さく注入し、wait が一度も呼ばれないことを固定する。
        let stub = RequestStub::new(&[(429, "rate limited", "{}")]);
        let waits = Cell::new(0usize);
        // 残予算 1s に対し最初のバックオフは BACKOFF_BASE(2s) で、待機が残予算を超える。
        let remaining = Duration::from_secs(1);
        let (status, _body, _headers) =
            retry_loop(|| stub.next(), |_| waits.set(waits.get() + 1), || remaining)?;
        assert_eq!(status, 429, "予算超過の待機をせず最後の 429 を返す");
        assert_eq!(stub.calls.get(), 1, "初回 request の後、待機せず即縮退する");
        assert_eq!(
            waits.get(),
            0,
            "deadline を跨ぐ待機はしない（一度も sleep しない）"
        );
        Ok(())
    }

    #[test]
    fn retry_loop_does_not_wait_when_budget_already_exhausted() -> crate::Result<()> {
        // 退行固定: 残予算 0（既に deadline 超過）なら、どんな短い待機でも入れずに即縮退する。
        let stub = RequestStub::new(&[(429, "rate limited", r#"{"retry-after":["1"]}"#)]);
        let waits = Cell::new(0usize);
        let (status, _body, _headers) = retry_loop(
            || stub.next(),
            |_| waits.set(waits.get() + 1),
            || Duration::ZERO,
        )?;
        assert_eq!(status, 429);
        assert_eq!(stub.calls.get(), 1);
        assert_eq!(waits.get(), 0);
        Ok(())
    }

    #[test]
    fn extract_budget_exhausted_is_false_before_and_true_after_deadline() {
        // 退行固定（deadline 判定の純粋部分）: 抽出フェーズ開始からの経過が EXTRACT_BUDGET 未満なら未超過、
        // 以上なら超過。adapter の extract_phase_start を直接操作して実時計に依存せず固定する。
        let adapter = GithubModelsExtractAdapter::new();
        // 開始時刻を「予算ちょうど未満」過去に設定（未超過）。
        let just_within = std::time::Instant::now()
            .checked_sub(EXTRACT_BUDGET - Duration::from_secs(1))
            .expect("instant within range");
        adapter.extract_phase_start.set(Some(just_within));
        assert!(
            !adapter.extract_budget_exhausted(),
            "予算未満は未超過（抽出を続ける）"
        );
        // 開始時刻を「予算超過」過去に設定（超過）。
        let past_budget = std::time::Instant::now()
            .checked_sub(EXTRACT_BUDGET + Duration::from_secs(1))
            .expect("instant within range");
        adapter.extract_phase_start.set(Some(past_budget));
        assert!(
            adapter.extract_budget_exhausted(),
            "予算超過は以降の抽出を skip する"
        );
    }

    #[test]
    fn extract_phase_start_is_anchored_on_first_query() {
        // 退行固定: 起点未確定の adapter は最初の予算問い合わせで起点を確定し、その時点では未超過（経過 ~0）。
        let adapter = GithubModelsExtractAdapter::new();
        assert!(adapter.extract_phase_start.get().is_none(), "初期は未確定");
        assert!(
            !adapter.extract_budget_exhausted(),
            "確定直後は経過 ~0 で未超過"
        );
        assert!(
            adapter.extract_phase_start.get().is_some(),
            "問い合わせで起点が確定する"
        );
    }

    #[test]
    fn pacing_interval_stays_under_per_minute_rate_limit() {
        // 退行固定（レート上限回避）: 本間隔は GitHub Models 無料枠の typical per-minute 上限（~15 req/min）を
        // 確実に下回ること。間隔から逆算した req/min（60s ÷ 間隔）が安全側（≤ ~10 req/min）であることを固定する。
        // これより短い間隔へ戻すと CI 実走で観測された HTTP 429 継続が再発する。
        let secs = MIN_REQUEST_INTERVAL.as_secs();
        assert!(
            secs >= 6,
            "間隔 {secs}s: per-minute 上限を下回るため 6s 以上が必要"
        );
        let req_per_min = 60 / secs;
        assert!(
            req_per_min <= 10,
            "{req_per_min} req/min: ~15 req/min 上限へ安全側で収める（≤10 req/min）"
        );
    }

    #[test]
    fn thirty_packages_pacing_fits_within_extract_budget() {
        // 退行固定（予算内）: 差分 30 パッケージ分を本間隔で直列に投げても、総ペーシング時間は抽出フェーズ予算
        // （EXTRACT_BUDGET = 35 分）に収まる。間隔拡大が予算を食い破らないことを固定する（30 件 × 8s = 240s ≪ 35 分）。
        const PACKAGES: u32 = 30;
        let total_pacing = MIN_REQUEST_INTERVAL * PACKAGES;
        assert!(
            total_pacing < EXTRACT_BUDGET,
            "30 パッケージの総ペーシング {total_pacing:?} は抽出予算 {EXTRACT_BUDGET:?} 内に収まる"
        );
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
    fn user_prompt_truncates_over_limit_seed_notes() {
        // 退行固定: 上限超の seed ノートは user prompt へ載せる前に切り詰める（HTTP 413 回避の核）。
        // truncate 印で終わり、本体が MAX_NOTES_CHARS 以内であることを確認する。
        let long_notes = "x".repeat(MAX_NOTES_CHARS + 9000);
        let request = ExtractRequest {
            name: "pkg".to_string(),
            old: None,
            new: None,
            repo: None,
            homepage: None,
            changelog: None,
            seed_notes: Some(RawReleaseNotes {
                text: long_notes,
                notes_url: "https://github.com/o/r/releases".to_string(),
            }),
        };
        let prompt = user_prompt(&request);
        assert!(prompt.ends_with(TRUNCATION_MARKER));
        // seed 本体（"x" 連続）は MAX_NOTES_CHARS 以内へ切られている。
        let x_count = prompt.chars().filter(|c| *c == 'x').count();
        assert_eq!(x_count, MAX_NOTES_CHARS);
    }

    #[test]
    fn agent_loop_fetches_then_summarizes_with_schema() -> crate::Result<()> {
        // 退行固定（エージェントループの主経路）: 初回 tool ターンで AI が fetch_url を要求 → adapter が fetch を
        // 実行（SSRF 許可 host）→ 再 request → AI がツールを出さない → 最終 schema ターンで構造化変更を返す。
        // 実 model/network/curl を呼ばず request/fetch closure を注入して規約を固定する。
        let request = request_with("neovim", Some("neovim/neovim"), None);
        let _allowed = allowed_fetch_hosts(Some("neovim/neovim"), None, None);

        let calls = Cell::new(0usize);
        let fetched = Cell::new(0usize);
        // 1 回目: tool_call で fetch_url を要求。2 回目（最終）: 構造化変更を返す。
        let model = |_body: &str| -> crate::Result<String> {
            let n = calls.get();
            calls.set(n + 1);
            if n == 0 {
                Ok(serde_json::json!({
                    "choices": [{
                        "finish_reason": "tool_calls",
                        "message": {
                            "content": "",
                            "tool_calls": [{
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "fetch_url",
                                    "arguments": "{\"url\":\"https://github.com/neovim/neovim/releases\"}"
                                }
                            }]
                        }
                    }]
                })
                .to_string())
            } else {
                Ok(serde_json::json!({
                    "choices": [{
                        "finish_reason": "stop",
                        "message": {
                            "content": "{\"changes\":[{\"category\":\"feature\",\"text\":\"新機能\",\"ref\":null}]}"
                        }
                    }]
                })
                .to_string())
            }
        };
        let fetch = |url: &str| -> crate::Result<Option<String>> {
            fetched.set(fetched.get() + 1);
            // 許可 host の URL なら fetch 成功を模す。
            assert_eq!(url, "https://github.com/neovim/neovim/releases");
            Ok(Some("## Features\n- new thing".to_string()))
        };

        let outcome = agent_loop(&request, model, fetch)?;
        // 1: tool ターン(fetch 要求) → 2: ツール無し（読了）でループ脱出 → 3: 最終 schema ターンで構造化変更。
        assert_eq!(
            calls.get(),
            3,
            "tool ターン + 読了ターン + 最終 schema ターン = 3 model 呼び出し"
        );
        assert_eq!(fetched.get(), 1, "AI が要求した 1 件を fetch する");
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(outcome.items[0].category, ChangeCategory::Feature);
        // 採用取得元（provenance）: AI が成功 fetch した URL を `origin=ai-discovered` 学習用に返す。
        assert_eq!(
            outcome.source_url.as_deref(),
            Some("https://github.com/neovim/neovim/releases")
        );
        Ok(())
    }

    #[test]
    fn agent_loop_skips_disallowed_url_but_returns_not_allowed() -> crate::Result<()> {
        // 退行固定（SSRF）: AI がノート本文由来の許可外 host を fetch_url で要求しても、注入された fetch が
        // None（fetch_allowed_note が host 不一致で fetch しない想定）を返し、ループは「not allowed」を AI へ
        // 返す。adapter は許可外 host へ実 fetch しない（ここでは fetch closure が呼ばれても host 判定で None）。
        let request = request_with("pkg", Some("o/r"), None);
        let allowed = allowed_fetch_hosts(Some("o/r"), None, None);
        let model = |_body: &str| -> crate::Result<String> {
            // 常に許可外 URL の fetch を 1 回要求し続けるが、反復上限で最終ターンへ抜ける。
            Ok(serde_json::json!({
                "choices": [{
                    "finish_reason": "tool_calls",
                    "message": {
                        "content": "",
                        "tool_calls": [{
                            "id": "c",
                            "type": "function",
                            "function": {
                                "name": "fetch_url",
                                "arguments": "{\"url\":\"https://evil.example/x\"}"
                            }
                        }]
                    }
                }]
            })
            .to_string())
        };
        // fetch_allowed_note と同じ SSRF 判定を模す closure: 許可 host 集合に無い URL は fetch せず None。
        let fetch = |url: &str| -> crate::Result<Option<String>> {
            if crate::update_history::domain::validate::fetch_host_allowed(url, &allowed) {
                Ok(Some("notes".to_string()))
            } else {
                Ok(None)
            }
        };
        // 最終ターンは空 changes を返す（model は常に tool_calls を返すが、反復上限後に final_body が呼ばれ、
        // ここでも同じ tool_calls JSON が返るため content 由来の changes は無く空になる）。
        let outcome = agent_loop(&request, model, fetch)?;
        assert!(
            outcome.items.is_empty(),
            "許可外 fetch しか得られなければ空配列へ縮退"
        );
        // 許可外 fetch しか得られなければ採用取得元も無い（許可外 URL を provenance へ学習しない）。
        assert!(
            outcome.source_url.is_none(),
            "許可外 fetch は採用取得元にしない（SSRF: 学習しない）"
        );
        Ok(())
    }

    #[test]
    fn agent_loop_bounds_tool_iterations() -> crate::Result<()> {
        // 退行固定（有界反復）: AI が毎ターン fetch_url を要求し続けても、tool ターンは MAX_TOOL_ITERATIONS 回
        // で打ち切り、最終要約ターンへ移る。model 呼び出し総数 = MAX_TOOL_ITERATIONS + 1（最終ターン）。
        let request = request_with("pkg", Some("o/r"), None);
        let calls = Cell::new(0usize);
        let model = |_body: &str| -> crate::Result<String> {
            calls.set(calls.get() + 1);
            Ok(serde_json::json!({
                "choices": [{
                    "finish_reason": "tool_calls",
                    "message": {
                        "content": "",
                        "tool_calls": [{
                            "id": "c",
                            "type": "function",
                            "function": {
                                "name": "fetch_url",
                                "arguments": "{\"url\":\"https://github.com/o/r/releases\"}"
                            }
                        }]
                    }
                }]
            })
            .to_string())
        };
        let fetch = |_url: &str| -> crate::Result<Option<String>> { Ok(Some("notes".to_string())) };
        let _ = agent_loop(&request, model, fetch)?;
        assert_eq!(
            calls.get(),
            (MAX_TOOL_ITERATIONS + 1) as usize,
            "tool 反復は上限で打ち切り、最終ターンを 1 回行う"
        );
        Ok(())
    }

    #[test]
    fn agent_loop_caps_tool_calls_per_turn() -> crate::Result<()> {
        // 退行固定（1 ターン fetch 件数上限）: AI が 1 レスポンスで大量の fetch_url を並べても、実行する fetch は
        // MAX_TOOL_CALLS_PER_TURN 件で打ち切る（残りは skipped をツール結果に）。
        let request = request_with("pkg", Some("o/r"), None);
        let fetched = Cell::new(0usize);
        let calls = Cell::new(0usize);
        let model = |_body: &str| -> crate::Result<String> {
            let n = calls.get();
            calls.set(n + 1);
            if n == 0 {
                // 1 ターンで MAX_TOOL_CALLS_PER_TURN + 3 件の fetch を要求する。
                let tool_calls: Vec<serde_json::Value> = (0..(MAX_TOOL_CALLS_PER_TURN + 3))
                    .map(|i| {
                        serde_json::json!({
                            "id": format!("c{i}"),
                            "type": "function",
                            "function": {
                                "name": "fetch_url",
                                "arguments": "{\"url\":\"https://github.com/o/r/releases\"}"
                            }
                        })
                    })
                    .collect();
                Ok(serde_json::json!({
                    "choices": [{
                        "finish_reason": "tool_calls",
                        "message": { "content": "", "tool_calls": tool_calls }
                    }]
                })
                .to_string())
            } else {
                Ok(serde_json::json!({
                    "choices": [{ "finish_reason": "stop", "message": { "content": "{\"changes\":[]}" } }]
                })
                .to_string())
            }
        };
        let fetch = |_url: &str| -> crate::Result<Option<String>> {
            fetched.set(fetched.get() + 1);
            Ok(Some("notes".to_string()))
        };
        let _ = agent_loop(&request, model, fetch)?;
        assert_eq!(
            fetched.get(),
            MAX_TOOL_CALLS_PER_TURN,
            "1 ターンの fetch は上限で打ち切る"
        );
        Ok(())
    }

    #[test]
    fn agent_loop_propagates_model_error() {
        // 退行固定（縮退）: model 呼び出しが Err（レート枯渇・HTTP エラー・curl 失敗）なら、エージェントループは
        // その Err を伝播し、呼び出し側（extract_change_items）が空配列へ縮退する。
        let request = request_with("pkg", Some("o/r"), None);
        let model = |_body: &str| -> crate::Result<String> { anyhow::bail!("rate limited") };
        let fetch = |_url: &str| -> crate::Result<Option<String>> { Ok(None) };
        assert!(agent_loop(&request, model, fetch).is_err());
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

    #[test]
    fn has_sufficient_seed_requires_nonblank_seed_text() {
        // 経路分岐の判定: seed が Some でかつ本文非空のときだけ summarize_only 経路（要約のみ）へ倒す。
        // seed 無し・空白だけの seed は探索経路（agent_loop）へ倒す。
        assert!(has_sufficient_seed(&request_with(
            "pkg",
            Some("o/r"),
            Some("## Fixes\n- crash")
        )));
        assert!(!has_sufficient_seed(&request_with(
            "pkg",
            Some("o/r"),
            None
        )));
        assert!(!has_sufficient_seed(&request_with(
            "pkg",
            Some("o/r"),
            Some("   \n  ")
        )));
    }

    #[test]
    fn summarize_only_calls_model_once_without_offering_tools() -> crate::Result<()> {
        // 退行固定（レート逓減の核）: seed が十分にあるとき summarize_only は **tools を与えず 1 回だけ** model を
        // 呼び、seed を根拠に構造化変更を返す。GitHub Models 呼び出しは 1 回（探索しない）で、リクエストボディに
        // tools/tool_choice を含まず response_format（strict schema）を強制することを固定する。
        let request = request_with(
            "neovim",
            Some("neovim/neovim"),
            Some("## Features\n- new thing"),
        );
        let calls = Cell::new(0usize);
        let model = |body: &str| -> crate::Result<String> {
            calls.set(calls.get() + 1);
            // summarize_only のリクエストボディは tools を提示せず schema を強制する。
            let value: serde_json::Value = serde_json::from_str(body).expect("body is valid JSON");
            assert!(
                value["tools"].is_null(),
                "summarize_only に tools は付けない"
            );
            assert!(
                value["tool_choice"].is_null(),
                "summarize_only に tool_choice は付けない"
            );
            assert_eq!(value["response_format"]["type"], "json_schema");
            // seed ノートが user メッセージへ載っていることを確認する（要約の根拠）。
            let messages = value["messages"].as_array().expect("messages array");
            let user = messages
                .iter()
                .find(|m| m["role"] == "user")
                .expect("user message");
            assert!(
                user["content"]
                    .as_str()
                    .is_some_and(|c| c.contains("new thing")),
                "user メッセージに seed ノートが載る"
            );
            Ok(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "{\"changes\":[{\"category\":\"feature\",\"text\":\"新機能\",\"ref\":null}]}"
                    }
                }]
            })
            .to_string())
        };
        let outcome = summarize_only(&request, model)?;
        assert_eq!(calls.get(), 1, "seed 有→要約のみで model 呼び出しは 1 回");
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(outcome.items[0].category, ChangeCategory::Feature);
        // summarize_only 経路は採用取得元を学習しない（ai-discovered は探索経路でのみ更新）。
        assert!(
            outcome.source_url.is_none(),
            "summarize_only は source_url を学習しない（既存 source 据え置き）"
        );
        Ok(())
    }

    #[test]
    fn summarize_only_keeps_hallucination_prevention_when_seed_yields_no_changes()
    -> crate::Result<()> {
        // 退行固定（ハルシネーション防止）: seed を根拠にしても LLM が「該当変更なし」を返せば空配列になる
        // （無ければ空という契約は tools の有無に依らない）。1 回だけ呼び、空 changes を空 items へ通す。
        let request = request_with("pkg", Some("o/r"), Some("内部リファクタのみ"));
        let calls = Cell::new(0usize);
        let model = |_body: &str| -> crate::Result<String> {
            calls.set(calls.get() + 1);
            Ok(serde_json::json!({
                "choices": [{ "message": { "content": "{\"changes\":[]}" } }]
            })
            .to_string())
        };
        let outcome = summarize_only(&request, model)?;
        assert_eq!(calls.get(), 1);
        assert!(outcome.items.is_empty(), "根拠が無ければ空配列へ縮退する");
        assert!(outcome.source_url.is_none());
        Ok(())
    }

    #[test]
    fn summarize_only_propagates_model_error() {
        // 退行固定（縮退）: summarize_only の model 呼び出しが Err なら、その Err を伝播し呼び出し側
        // （extract_change_items）が空配列へ縮退する。
        let request = request_with("pkg", Some("o/r"), Some("notes"));
        let model = |_body: &str| -> crate::Result<String> { anyhow::bail!("rate limited") };
        assert!(summarize_only(&request, model).is_err());
    }

    #[test]
    fn agent_loop_path_explores_when_seed_absent() -> crate::Result<()> {
        // 退行固定（探索経路）: seed が無いときは agent_loop が fetch_url で探索する（複数 model 呼び出し）。
        // この振り分けは has_sufficient_seed が担い、seed 無しでは探索経路へ倒ることを agent_loop 経由で固定する。
        let request = request_with("neovim", Some("neovim/neovim"), None);
        assert!(!has_sufficient_seed(&request), "seed 無しは探索経路");
        let calls = Cell::new(0usize);
        let model = |_body: &str| -> crate::Result<String> {
            let n = calls.get();
            calls.set(n + 1);
            if n == 0 {
                Ok(serde_json::json!({
                    "choices": [{
                        "finish_reason": "tool_calls",
                        "message": {
                            "content": "",
                            "tool_calls": [{
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "fetch_url",
                                    "arguments": "{\"url\":\"https://github.com/neovim/neovim/releases\"}"
                                }
                            }]
                        }
                    }]
                })
                .to_string())
            } else {
                Ok(serde_json::json!({
                    "choices": [{
                        "message": { "content": "{\"changes\":[]}" }
                    }]
                })
                .to_string())
            }
        };
        let fetch =
            |_url: &str| -> crate::Result<Option<String>> { Ok(Some("## Notes".to_string())) };
        let outcome = agent_loop(&request, model, fetch)?;
        assert!(
            calls.get() > 1,
            "seed 無し探索経路は複数 model 呼び出しを行う（要約のみの 1 回ではない）"
        );
        // 探索で成功 fetch した URL は採用取得元として返る（ai-discovered 学習経路）。
        assert_eq!(
            outcome.source_url.as_deref(),
            Some("https://github.com/neovim/neovim/releases")
        );
        Ok(())
    }
}

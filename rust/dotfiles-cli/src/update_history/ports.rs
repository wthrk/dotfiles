//! `update-history` application が外部境界へ要求する port 契約。
//!
//! 各 trait は「何を必要とするか」の意図宣言だけを持ち、具体依存（nix プロセス、brew tap rev の
//! formula/cask 解析、リリースノート HTTP 取得、GitHub Models 呼び出し、TOML ファイル I/O）は持たない。
//! 境界型は domain 型（`VersionDelta` / `ChangeItem` / `UpdateEntry`）に限定し、SDK 型・パーサ・prompt
//! 文言・利用者向け文言は adapter（`adapters/nix.rs`・`adapters/brew.rs`・`adapters/notes.rs`・
//! `adapters/github_models.rs`・`adapters/toml_store.rs`・`adapters/report.rs`）へ閉じる。各 trait の実体
//! 実装はそれら adapter module が担う。

use std::collections::BTreeMap;

use super::domain::diff::{DeltaSource, NixPackage, VersionDelta};
use super::domain::registry::NotesSourceRegistry;
use super::domain::view::HistoryView;
use super::domain::wire::{ChangeItem, UpdateEntry};
use crate::Result;

/// nix 参照構成の宣言パッケージ name→[`NixPackage`] マップを old/new それぞれ取得する capability 契約
/// （外部機能: nix eval 結果の取得）。
///
/// nightly が欲しいのは「どの宣言パッケージが old→new で版変化したか」と「各パッケージの当該版の
/// リリースノート取得先」であり、いずれも `nix eval` で評価時属性（`pname`/`version` と
/// `meta.changelog`/`meta.homepage`）として数秒で取れる。closure を実体化（`diff-closures`）してフル
/// closure を 2 回ビルドする必要はない。caller（application）は old/new の [`NixPackage`] マップを受け取り、
/// 比較（[`super::domain::diff::diff_versions`]）と差分種別・ノート取得先の運搬を domain rule に委ねる。
/// implementor は ci-ref の old/new lock で eval 済みの `{ name: { version, notes_source } }` JSON を取得して
/// `BTreeMap` へ翻訳するだけを担う。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait NixVersionPort {
    /// bump 前（old lock）の宣言パッケージ name→[`NixPackage`] マップを返す。
    fn old_versions(&self) -> Result<BTreeMap<String, NixPackage>>;

    /// bump 後（new lock）の宣言パッケージ name→[`NixPackage`] マップを返す。
    fn new_versions(&self) -> Result<BTreeMap<String, NixPackage>>;
}

/// Homebrew tap rev 間の version 差分を取得する capability 契約（外部機能: brew tap 解析）。
///
/// implementor は old/new tap rev が提供する formula/cask の version を決定論的に算出し
/// （ライブ `brew` 問い合わせはしない）、[`VersionDelta`] 列へ翻訳する。どの tap を対象にするか、
/// どの cask を無人更新対象から外すかの方針は caller/上位設定の責務であり、port は差分取得契約に限定する。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait BrewVersionDiffPort {
    /// old/new tap rev 間の brew version 差分を返す。
    fn diff_brew_versions(&self, old_rev: &str, new_rev: &str) -> Result<Vec<VersionDelta>>;
}

/// 更新パッケージの生リリースノートを取得する capability 契約（外部機能: ノート取得）。
///
/// caller は対象パッケージと version 範囲、差分の出所（[`DeltaSource`]）、nix eval 由来の GitHub
/// `owner/repo`（`repo`）と changelog URL（`notes_source`）を渡す。implementor は出所ごとに異なる取得元を
/// 選ぶ: nix eval 由来は delta が運ぶ `repo` から **GitHub Releases API で `(old, new]` 範囲のリリースノート**
/// を取得し（空振り時は `notes_source`（`meta.changelog`/`meta.homepage`）の changelog raw へフォールバック）、
/// brew tap 由来は cask 定義の `Casks/<letter>/<name>.rb` から生ノートテキストを取得して返す。出所を渡すのは、
/// nix と brew で取得規則が異なり、同一規則で引くと誤った URL（例: nix package を cask レイアウトで引いて 404）
/// になるためである。いずれの取得元も解決不能・取得不能なら `None` を返し（フォールバックは version + URL のみ）、
/// ノートの構造化や要約は行わない。生ノート・`repo`・`notes_source` はいずれも信頼境界外であり、取得は許可
/// ホスト https に限定し、後段の機械バリデートでも守る。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait NotesPort {
    /// 対象パッケージの `(old, new]` 範囲の生リリースノートを、差分の出所に応じた取得元から取得する。
    ///
    /// `source` は nix eval 由来か brew tap 由来かを示し、implementor はこれで取得元 / 解決規則を振り分ける。
    /// `repo` は nix eval 由来 delta が運ぶ GitHub `owner/repo`（Releases API の一次取得元。brew・github 由来
    /// 不明では `None`）。`notes_source` は changelog URL（Releases API 空振り時のフォールバック取得元。
    /// brew では `None`）。`old`/`new` は Releases API で `(old, new]` 範囲のリリースを絞るための version。
    /// implementor はノート本文取得と URL 確定だけを担い、変更概要の意味づけや severity 算出は行わない。
    fn fetch_release_notes(
        &self,
        name: &str,
        source: DeltaSource,
        repo: Option<String>,
        notes_source: Option<String>,
        old: Option<String>,
        new: Option<String>,
    ) -> Result<Option<RawReleaseNotes>>;

    /// レジストリに保存済みの取得元 URL を **直接** fetch して生ノートを取得する（再利用フロー専用）。
    ///
    /// 利用者要件 (4): 前回 record でレジストリに学習した取得元（`source`）があれば、次回はそれを直接
    /// fetch して再利用し、機械解決・AI 探索を一切しない（AI 探索を新規/未知/自己修復のみへ限定して
    /// GitHub Models のレート消費を逓減させる）。`url` はレジストリ由来（repo 管理・レビュー対象だが、
    /// AI-discovered で書かれた URL は元を辿れば AI 由来）であり、implementor は取得前に必ず host allowlist
    /// （`is_allowed_url`）+ `-L` 無し・`--max-redirs 0`・https を機械適用する（既存 fetch 経路と同一）。
    /// 取得失敗・空本文・許可外 host はいずれも `None`（呼び出し側は **自己修復**として機械解決 → AI 探索へ
    /// フォールバックする）。`notes_url`（記録に残す URL）は取得した `url` をそのまま採る。
    fn fetch_notes_from_source(&self, url: &str) -> Result<Option<RawReleaseNotes>>;
}

/// 取得済み生リリースノートと参照 URL の境界型。
///
/// `text` は信頼境界外の生テキスト（prompt injection 源）であり、LLM 抽出後に機械バリデートする前提で運ぶ。
/// `notes_url` は **記録・表示に残す**ノート参照 URL（人間が辿れるページ。例: `github.com/{o}/{r}/releases`）。
///
/// `refetch_url` は **同じ生ノート本文を後から raw 取得し直せる URL**（あれば）。provenance レジストリへ
/// 再利用 source として学習してよいのはこちらだけである（finding 3369076722）。`notes_url`（表示用 HTML ページ
/// 等）を再利用 source にすると、次回 [`NotesPort::fetch_notes_from_source`] がその HTML/JSON ページを raw curl
/// して seed にしてしまい、要約が空/不正確になる。`refetch_url` は「raw 取得で同じ本文が返る URL」のときだけ
/// `Some`（例: raw changelog ファイル・cask `.rb` 生ファイル）にし、Releases API の range/tag 取得のように
/// 単一 raw URL で本文を再現できない取得では `None` にする（その場合 record は再利用 source を学習せず、次回も
/// 機械解決し直す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawReleaseNotes {
    /// `(old, new]` 範囲の生リリースノート本文（信頼境界外）。
    pub(crate) text: String,
    /// 記録・表示に残すノート参照 URL（人間が辿れるページ。raw 再取得できるとは限らない）。
    pub(crate) notes_url: String,
    /// 同じ本文を raw 取得し直せる URL（あれば）。provenance の再利用 source に学習してよいのはこれだけ
    /// （finding 3369076722）。raw 再現できない取得（Releases API range/tag 等）では `None`。
    pub(crate) refetch_url: Option<String>,
}

/// AI エージェントが「適切なリリースノートのソースを自分で探して fetch して読み」構造化変更リストを抽出する
/// ための、1 パッケージ分の入力境界（信頼境界内 = eval メタ由来のヒント）。
///
/// リリースノートの場所は機械的に一律取得できないため、抽出は **AI エージェントに適切なノートを取得させて
/// 要約させる**方式へ作り直した。adapter（GitHub Models tool-use ループ）はこのヒントから、パッケージの正規
/// ドメインに限定した fetch 許可ホスト集合を組み立て（[`super::domain::validate::allowed_fetch_hosts`]）、AI が
/// 要求した `fetch_url` をその集合内 https に限って実行する（SSRF 防御）。`seed_notes` は機械解決で先に取れた
/// 生ノート（あれば会話の初期材料として与える。AI は更に自分で fetch して補える）。
///
/// 各フィールドはすべて eval（信頼境界内）由来のヒントであり、ノート本文（信頼境界外）では拡張しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractRequest {
    /// パッケージ名（AI への user prompt に載せる識別子）。
    pub(crate) name: String,
    /// 更新前 version（`added` では `None`）。
    pub(crate) old: Option<String>,
    /// 更新後 version（`removed` では `None`）。
    pub(crate) new: Option<String>,
    /// GitHub `owner/repo`（eval 抽出。fetch 許可ホスト集合と AI ヒント）。
    pub(crate) repo: Option<String>,
    /// homepage URL（`meta.homepage`。fetch 許可ホスト集合と AI ヒント）。
    pub(crate) homepage: Option<String>,
    /// changelog URL（`meta.changelog`/`meta.homepage`。fetch 許可ホスト集合と AI ヒント）。
    pub(crate) changelog: Option<String>,
    /// 機械解決で先に取得できた生ノート（あれば AI へ初期材料として与える。信頼境界外）。無ければ `None`。
    pub(crate) seed_notes: Option<RawReleaseNotes>,
}

/// AI エージェント抽出の結果（構造化変更リスト + AI が採用した取得元 URL）。
///
/// `items` は機械バリデート前の構造化変更（信頼境界外）。`source_url` は AI が会話中に実際に fetch して
/// 採用した取得元 URL（provenance として `origin=ai-discovered` でレジストリへ学習する経路。fetch
/// していない・採用できなかったときは `None`）。SSRF 検査を通った fetch の URL だけを運ぶが、レジストリへ
/// 書く前に呼び出し側（`record`）が host allowlist を再適用する（信頼境界外 URL を学習しないため）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ExtractOutcome {
    /// 抽出した構造化変更リスト（機械バリデート前。信頼境界外）。
    pub(crate) items: Vec<ChangeItem>,
    /// AI が採用した取得元 URL（あれば。`origin=ai-discovered` 学習経路。無ければ `None`）。
    pub(crate) source_url: Option<String>,
}

/// AI エージェントにノートを取得・要約させて構造化変更リストを抽出する capability 契約（外部機能: LLM エージェント）。
///
/// implementor（GitHub Models tool-use ループ）は [`ExtractRequest`] のヒント（パッケージ名・old→new・
/// homepage/repo/changelog）から **AI 自身に適切なリリースノートのソースを探させ、`fetch_url` ツールで取得・
/// 読解させ**、構造化変更（category + text + ref）の配列を返させる。caller は抽出結果を機械バリデート
/// （schema/enum/長さ/host）してから記録に使う。severity はこの抽出結果でなく category enum から機械算出する
/// ため、LLM 出力はマージ判断に使わない（injection 耐性）。
///
/// SSRF 防御は implementor の責務: fetch 許可ホスト集合は **eval メタ由来のヒント host だけ**から組み立て、AI が
/// 要求した URL の host が集合外なら fetch せずツール結果として拒否を返す（ノート本文由来 URL を無検証で
/// fetch しない）。fetch は許可ホスト https に限定し、取得テキストは truncate・反復回数は有界にする。
///
/// 抽出フェーズ全体の wall-clock 予算（[`Self::extract_budget_exhausted`]）も契約に含む。複数パッケージを
/// 順に抽出する caller（application）は、各抽出の前に予算超過を問い合わせ、超過後は LLM 抽出を skip して
/// version-only へ縮退させる。これは「外部 I/O にどれだけ時間を使ってよいか」という抽出 I/O の予算境界であり、
/// 残りパッケージを LLM 抽出するか version-only に倒すかという停止条件の判断は caller（application）が担う。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait ChangeExtractPort {
    /// AI エージェントにノートを取得・要約させて構造化変更リストを抽出し、AI が**採用した取得元 URL**を併せて返す。
    ///
    /// implementor は与えたヒントから AI に適切なノートを fetch・読解させ、取得した実ノートのみを根拠として
    /// （ハルシネーション禁止）構造化変更を返す。根拠が無ければ空配列を返す。加えて、AI が会話中に実際に
    /// fetch し採用した取得元 URL（あれば最後に成功した fetch URL）を [`ExtractOutcome::source_url`] として返す。
    /// これは provenance レジストリへ `origin=ai-discovered` の取得元として学習・再利用するための経路であり
    /// （利用者要件 (3)/(4)）、fetch していない・採用できなかった場合は `None` を返す。返す URL は SSRF 検査
    /// （許可ホスト集合内 https）を通過した fetch のものだけであり、呼び出し側は記録前に host allowlist を再適用する。
    fn extract_change_items(&self, request: &ExtractRequest) -> Result<ExtractOutcome>;

    /// 抽出フェーズ全体の wall-clock 予算を使い切ったか（`true` なら以降の LLM 抽出を skip すべき）。
    ///
    /// implementor は抽出フェーズ開始時刻を起点に、総時間予算を超過したかだけを返す（外部 I/O はしない）。
    /// caller（application）はこれを各パッケージ抽出の前に問い合わせ、超過後は LLM 抽出を呼ばず version-only
    /// へ縮退させる（record 全体は success を維持し、skip 件数を診断ログで明示する）。予算を設ける理由は、
    /// 全件持続 429 のような最悪ケースで抽出が record job timeout（60分）へ接近・超過し、後続 job（PR 起票）が
    /// 止まって無人 nightly が停止するのを構造的に防ぐためである。
    fn extract_budget_exhausted(&self) -> bool;
}

/// 更新履歴 TOML を読み書きする capability 契約（外部機能: TOML ファイル I/O）。
///
/// implementor は `docs/update-history/<YYYY-MM>.toml` の read/append を担う。append は既存エントリを
/// 保ったまま新 [`UpdateEntry`] を追記する（1 ファイルに 1 日複数件可）。serde derive を介した
/// encode/decode の具体実装は adapter に閉じ、domain は `toml` クレートへ依存しない。catch-up の
/// チェーン連結や表示時集約は application/domain の責務であり、store は単純な永続化境界に限定する。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait HistoryStorePort {
    /// 指定履歴ファイルの全エントリを読み出す（不存在なら空 Vec）。
    fn read_entries(&self) -> Result<Vec<UpdateEntry>>;

    /// 新エントリを既存履歴へ追記する（既存エントリは保持する）。
    fn append_entry(&self, entry: &UpdateEntry) -> Result<()>;
}

/// record の縮退・provenance 経路の診断サマリを出力する capability 契約（外部機能: 診断ログ出力）。
///
/// record use case は、抽出予算超過で version-only へ縮退した件数、概要付き/version-only の件数、provenance
/// 経路（registry-reused / mechanical / ai-discovered）の内訳を CI ログに残し、無人パイプラインが「token 失効や
/// レート枯渇で全件 version-only に静かに全滅した」「どの経路でノートを得たか」を観測できるようにする。これらは
/// **何を観測させるか**という意図であり、stderr への具体的な書き出し（`eprintln!`）は presentation/I/O であって
/// application の責務ではない。caller（application）は集計した件数を渡すだけで、出力先・整形は implementor
/// （adapter）に閉じる（application から concrete I/O を排除する）。各メソッドは観測専用で、失敗しても record の
/// 成否に影響させない（implementor は best-effort 出力に倒す）。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait RecordDiagnosticsPort {
    /// 抽出予算超過で LLM 抽出を skip し version-only へ縮退したパッケージ件数を診断する（件数 0 なら呼ばない）。
    fn report_budget_skipped(&self, skipped: usize);

    /// ノート取得・抽出フェーズの縮退サマリ（概要付き件数 / version-only 件数）と provenance 経路内訳
    /// （registry-reused / mechanical / ai-discovered）を診断する。対象 delta が 1 件もない夜は呼ばない。
    fn report_notes_summary(
        &self,
        summarized: usize,
        version_only: usize,
        registry_hits: usize,
        mechanical_found: usize,
        ai_discovered: usize,
    );
}

/// 集約済み履歴ビューを利用者向けに出力する capability 契約（外部機能: 端末 / JSON 出力）。
///
/// caller（show application）は catch-up 集約と severity 再算出を終えた [`HistoryView`] を渡し、
/// implementor は重要度連動の text または生 JSON へ翻訳して出力する。絵文字凡例・JSON key・整形・
/// プレーン表示（`text` をリンク化/実行しない injection 契約）といった presentation 仕様は adapter に閉じ、
/// caller は「集約済みビューを出力する」という意図だけを要求する。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait HistoryReportPort {
    /// 集約済み履歴ビューを利用者向け表示として書き出す。
    ///
    /// `json` が `true` のとき生データ（JSON）を、`false` のとき重要度連動の text を出力する。
    /// 表示順・絵文字・破壊的/セキュリティ先頭などの整形規則は implementor が決める。
    fn write_history(&self, view: &HistoryView, json: bool) -> Result<()>;
}

/// ノート取得元レジストリ（provenance の学習・再利用）を読み書きする capability 契約（外部機能: TOML ファイル I/O）。
///
/// 利用者要件 (3)/(4): record はパッケージごとにどこからノートを取得したか（取得元 URL + origin）を
/// repo 管理の TOML（`docs/update-history/notes-sources.toml`）へ保存し、次回以降はそれを参照して
/// 再利用し再探索しない。implementor は read（不存在なら空レジストリ）と write（決定論・名前昇順で全体を
/// 書き戻す）を担う。レジストリの参照優先・自己修復・origin 別の再探索要否は application/domain の責務であり、
/// store は単純な永続化境界（全体 read / 全体 write）に限定する。serde derive を介した TOML encode/decode の
/// 具体実装は adapter に閉じ、domain は `toml` クレートへ依存しない。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait NotesSourceRegistryPort {
    /// レジストリ全体を読み出す（不存在なら空レジストリ）。
    fn read_registry(&self) -> Result<NotesSourceRegistry>;

    /// レジストリ全体を書き戻す（決定論・名前昇順）。
    fn write_registry(&self, registry: &NotesSourceRegistry) -> Result<()>;
}

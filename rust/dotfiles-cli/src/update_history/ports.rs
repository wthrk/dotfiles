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
}

/// 取得済み生リリースノートと参照 URL の境界型。
///
/// `text` は信頼境界外の生テキスト（prompt injection 源）であり、LLM 抽出後に機械バリデートする前提で運ぶ。
/// `notes_url` は記録に残すノート URL。具体的な取得実装（HTTP・forge API）は adapter に閉じる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawReleaseNotes {
    /// `(old, new]` 範囲の生リリースノート本文（信頼境界外）。
    pub(crate) text: String,
    /// 記録に残すノート参照 URL。
    pub(crate) notes_url: String,
}

/// 生リリースノートから構造化変更リストを抽出する capability 契約（外部機能: LLM 抽出）。
///
/// implementor は GitHub Models 等で生ノートを `Vec<ChangeItem>`（category + text + ref）へ抽出する。
/// caller は抽出結果を機械バリデート（schema/enum/長さ/host）してから記録に使う。severity はこの抽出
/// 結果でなく category enum から機械算出するため、LLM 出力はマージ判断に使わない（injection 耐性）。
///
/// 抽出フェーズ全体の wall-clock 予算（[`Self::extract_budget_exhausted`]）も契約に含む。複数パッケージを
/// 順に抽出する caller（application）は、各抽出の前に予算超過を問い合わせ、超過後は LLM 抽出を skip して
/// version-only へ縮退させる。これは「外部 I/O にどれだけ時間を使ってよいか」という抽出 I/O の予算境界であり、
/// 残りパッケージを LLM 抽出するか version-only に倒すかという停止条件の判断は caller（application）が担う。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait ChangeExtractPort {
    /// 生リリースノートを構造化変更リストへ抽出する。
    ///
    /// implementor は与えた生ノートのみを根拠とし（ハルシネーション禁止）、根拠が無ければ空配列を返す。
    fn extract_change_items(&self, notes: &RawReleaseNotes) -> Result<Vec<ChangeItem>>;

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

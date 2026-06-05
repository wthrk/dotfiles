//! `update-history` application が外部境界へ要求する port 契約。
//!
//! 各 trait は「何を必要とするか」の意図宣言だけを持ち、具体依存（nix プロセス、brew tap rev の
//! formula/cask 解析、リリースノート HTTP 取得、GitHub Models 呼び出し、TOML ファイル I/O）は持たない。
//! 境界型は domain 型（`VersionDelta` / `ChangeItem` / `UpdateEntry`）に限定し、SDK 型・パーサ・prompt
//! 文言・利用者向け文言は adapter（`adapters/nix.rs`・`adapters/brew.rs`・`adapters/notes.rs`・
//! `adapters/github_models.rs`・`adapters/toml_store.rs`・`adapters/report.rs`）へ閉じる。各 trait の実体
//! 実装はそれら adapter module が担う。

use super::domain::diff::{DeltaSource, VersionDelta};
use super::domain::view::HistoryView;
use super::domain::wire::{ChangeItem, UpdateEntry};
use crate::Result;

/// nix クロージャ間の version 差分を取得する capability 契約（外部機能: nix プロセス実行）。
///
/// caller（application）は old/new closure path を決め、diff 実行順序を制御する。implementor は
/// `nix store diff-closures` を実行し、その出力を domain パーサへ通して [`VersionDelta`] 列へ翻訳する。
/// version 比較規則や差分種別の業務意味は domain rule に委ね、adapter は実行と翻訳に限定する。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait ClosureDiffPort {
    /// old/new closure 間の nix version 差分を返す。
    ///
    /// implementor は 2 つの closure store path を受け取り diff を実行する。closure 選択や
    /// 参照構成の決定は caller の責務であり、implementor は差分テキストの取得と翻訳だけを担う。
    fn diff_closures(&self, old_closure: &str, new_closure: &str) -> Result<Vec<VersionDelta>>;
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
/// caller は対象パッケージと version 範囲、そして差分の出所（[`DeltaSource`]）を渡す。implementor は
/// 出所ごとに異なるノート取得先（nix=forge releases / nixpkgs `meta.changelog`、brew=cask 定義の
/// `Casks/<letter>/<name>.rb`）を選び、`(old, new]` の生ノートテキストを取得して返す。出所を渡すのは、
/// nix と brew で取得先の base URL / 解決規則が異なり、同一規則で引くと誤った URL（例: nix package を cask
/// レイアウトで引いて 404）になるためである。取得不能時は `None` を返し（フォールバックは version + URL の
/// み）、ノートの構造化や要約は行わない。生ノートは信頼境界外であり、後段の機械バリデートで守る。
#[cfg_attr(test, mockall::automock)]
pub(crate) trait NotesPort {
    /// 対象パッケージの `(old, new]` 範囲の生リリースノートを、差分の出所に応じた取得先から取得する。
    ///
    /// `source` は nix クロージャ由来か brew tap 由来かを示し、implementor はこれで取得先 base / 解決規則を
    /// 振り分ける。`notes_url` は記録に残すノート参照 URL。implementor はノート本文取得と URL 確定だけを担い、
    /// 変更概要の意味づけや severity 算出は行わない。
    fn fetch_release_notes(
        &self,
        name: &str,
        source: DeltaSource,
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
#[cfg_attr(test, mockall::automock)]
pub(crate) trait ChangeExtractPort {
    /// 生リリースノートを構造化変更リストへ抽出する。
    ///
    /// implementor は与えた生ノートのみを根拠とし（ハルシネーション禁止）、根拠が無ければ空配列を返す。
    fn extract_change_items(&self, notes: &RawReleaseNotes) -> Result<Vec<ChangeItem>>;
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

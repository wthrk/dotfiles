//! `update-history` の `record` / `show` use case が扱う入力 command の domain model。
//!
//! CLI option の parse 方式・出力形式・ファイル解決手段は含めず、application が適用する対象
//! （diff する closure / tap rev、記録する時刻と参照構成、表示の絞り込み条件）だけを保持する。
//! use case 独自型を application 側に置かないため、入力境界も domain 値として固定する。

/// `record` use case の入力 command。
///
/// CI（nightly bump）が記録する nixpkgs リビジョン、適用時刻（RFC3339 文字列）、diff 対象の参照構成を
/// 保持する。nix version 差分（eval JSON）と brew 版差分（tap rev ファイル）の取得・ノート取得・LLM 抽出・
/// 追記先ファイルの解決手段は port 境界へ委譲し、本型は「何の rev・時刻・参照で記録するか」だけを表す。
/// eval ベース化により、以前 closure store path を保持していた `old_closure`/`new_closure` は不要になった
/// （nix 差分は eval JSON ファイルから adapter が取得する）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordCommand {
    /// brew 版差分の diff 元 rev 座標。現行の file ベース brew adapter は `--brew-diff` を使うため本値は
    /// 参照されない（port 契約互換のため保持。CI は nixpkgs rev を流用注入する）。
    pub(crate) old_rev: String,
    /// brew 版差分の diff 先 rev 座標。`old_rev` と同様に現行 adapter では未参照。
    pub(crate) new_rev: String,
    /// 記録する bump 前 nixpkgs リビジョン。
    pub(crate) nixpkgs_old: String,
    /// 記録する bump 後 nixpkgs リビジョン。
    pub(crate) nixpkgs_new: String,
    /// diff 対象の参照構成（例: `darwinConfigurations.<ref>`）。
    pub(crate) reference: String,
    /// 適用時刻（RFC3339。CI が `--at` で注入する文字列をそのまま記録する）。
    pub(crate) at: String,
}

/// `show` use case の入力 command。
///
/// 表示の絞り込み条件だけを保持する。履歴 source（`docs/update-history`）の解決手段や描画形式は
/// application/adapter の責務であり、本型は「どこまで遡り、どう絞り、生データを出すか」という
/// 表示意図だけを domain 値として表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShowCommand {
    /// 表示起点の nixpkgs リビジョン（`None` なら最新エントリまで）。
    pub(crate) rev: Option<String>,
    /// 表示するエントリ件数の上限（`None` なら無制限）。
    pub(crate) limit: Option<usize>,
    /// 生データ（JSON）で出力するか。
    pub(crate) json: bool,
    /// 宣言アプリだけでなく全パッケージを表示するか。
    pub(crate) all: bool,
}

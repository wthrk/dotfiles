//! `dotfiles update-history` の hexagonal module 境界と composition root。
//!
//! この機能は nightly bump で更新されたアプリの version 差分と「何が変わったか」の構造化変更リストを
//! `docs/update-history/<YYYY-MM>.toml` に記録（`record`）し、適用済み pin の記録を閲覧（`show`）する。
//!
//! 本 module は domain（wire 型・severity 機械算出・catch-up 集約・overall 見出し・diff パーサ・
//! URL/長さ機械バリデート・表示ビュー）と port 契約（diff(nix)/diff(brew)/note/llm/toml/report）、
//! adapter 実体（nix 実行・brew tap 解析・リリースノート取得・GitHub Models 抽出・TOML I/O・stdout 表示）、
//! application（`record` / `show` の `run_*`）を束ねる。composition root は adapter concrete の所有関係だけを
//! 確定し、`pub(in crate::update_history)` で具体を閉じる。CLI option は clap の型付けに限定し、use case の
//! 順序・diff/notes/LLM の実装詳細は application/adapter 以下へ閉じ込める。

/// adapter concrete modules を composition root からだけ到達できる範囲に閉じる。
mod adapters {
    mod brew;
    mod github_models;
    mod nix;
    mod notes;
    mod report;
    mod toml_store;

    pub(in crate::update_history) use brew::BrewTapDiffAdapter;
    pub(in crate::update_history) use github_models::GithubModelsExtractAdapter;
    pub(in crate::update_history) use nix::NixClosureDiffAdapter;
    pub(in crate::update_history) use notes::ReleaseNotesAdapter;
    pub(in crate::update_history) use report::{
        StdoutHistoryReportAdapter, WriterHistoryReportAdapter,
    };
    pub(in crate::update_history) use toml_store::TomlHistoryStoreAdapter;
}
mod application;
pub(crate) mod domain;
pub(crate) mod ports;

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use crate::Result;
use domain::commands::{RecordCommand, ShowCommand};

/// `docs/update-history` 配下のファイル名（`<YYYY-MM>.toml`）を解決する既定 source ディレクトリ名。
const HISTORY_SUBDIR: &str = "docs/update-history";

#[derive(Args)]
/// 更新履歴の記録（CI）と閲覧（利用者）を分けて公開する最上位 command。
pub(crate) struct UpdateHistoryOptions {
    #[command(subcommand)]
    command: UpdateHistoryCommand,
}

#[derive(Subcommand)]
/// CI が叩く記録 command と、利用者が叩く閲覧 command。
enum UpdateHistoryCommand {
    Record(RecordOptions),
    Show(ShowOptions),
}

#[derive(Args)]
/// nightly bump で更新されたアプリの version + 概要を 1 エントリ記録する option。
///
/// CI（network + GitHub Models）が叩く。old/new の nix closure を `diff-closures` で diff し（brew 版差分は
/// `--brew-diff` ファイルから読む）、各アプリの生ノートを取得して LLM で構造化抽出し、`--out` の月次 TOML へ
/// 追記する。`--at` は RFC3339 を注入する。
struct RecordOptions {
    /// diff 元の nix closure store path。
    #[arg(long)]
    old: String,
    /// diff 先の nix closure store path。
    #[arg(long)]
    new: String,
    /// brew 版差分の diff 元 rev（座標）。現行の brew adapter は `--brew-diff` ファイルを使うため本値は
    /// 参照されないが、port 契約は rev 座標を受けるため互換のため受け取る（CI は nixpkgs rev を流用注入する）。
    #[arg(long)]
    old_rev: String,
    /// brew 版差分の diff 先 rev（座標）。`--old-rev` と同様に現行 adapter では未参照。
    #[arg(long)]
    new_rev: String,
    /// 記録する bump 前 nixpkgs リビジョン。
    #[arg(long)]
    nixpkgs_old: String,
    /// 記録する bump 後 nixpkgs リビジョン。
    #[arg(long)]
    nixpkgs_new: String,
    /// diff 対象の参照構成（例: `darwinConfigurations.<ref>`）。
    #[arg(long)]
    reference: String,
    /// 適用時刻（RFC3339）。CI が `date -u +%FT%TZ` を注入する。
    #[arg(long)]
    at: String,
    /// 追記先の月次 TOML ファイル（`docs/update-history/<YYYY-MM>.toml`）。
    #[arg(long)]
    out: PathBuf,
    /// CI が old/new tap rev から事前算出した brew 版差分ファイル（`name<TAB>old<TAB>new`）。
    /// 未指定なら brew 差分は縮退して空。
    #[arg(long)]
    brew_diff: Option<PathBuf>,
    /// リリースノート URL の基底（`<notes_base><name>` を取得対象にする）。未指定ならノート取得は縮退して空。
    #[arg(long)]
    notes_base: Option<String>,
}

#[derive(Args)]
/// 適用済み pin 由来の更新履歴を閲覧する option。
///
/// `--rev` 起点からの catch-up 区間をアプリ単位で集約し、severity バッジ + 全体概要 + アプリ別変更リストを
/// 表示する。`--source` 省略時は `<config-dir>/docs/update-history` を読む。
struct ShowOptions {
    /// 表示起点の nixpkgs リビジョン（省略時は最新まで）。
    #[arg(long)]
    rev: Option<String>,
    /// 表示するエントリ件数の上限。
    #[arg(long)]
    limit: Option<usize>,
    /// 生データ（JSON）で出力する。
    #[arg(long)]
    json: bool,
    /// 宣言アプリだけでなく全パッケージを表示する。
    #[arg(long)]
    all: bool,
    /// 履歴を読む対象 source（ファイル/ディレクトリ）。省略時は `<config-dir>/docs/update-history`
    /// ディレクトリを既定 source とし、配下の全 `*.toml` 月次ファイルを連結して読む。
    #[arg(long)]
    source: Option<PathBuf>,
}

/// CLI で parse 済みの `dotfiles update-history` command を composition root へ渡す。
///
/// CLI 入口は command 定義と option 変換だけを担い、adapter concrete 生成と use case 結線は
/// composition root（[`run_record`] / [`run_show`]）へ閉じる。
pub(crate) fn run(options: UpdateHistoryOptions) -> Result<()> {
    match options.command {
        UpdateHistoryCommand::Record(options) => run_record(options),
        UpdateHistoryCommand::Show(options) => run_show(options),
    }
}

/// record 経路の composition root: adapter concrete を結線し record use case を駆動する。
fn run_record(options: RecordOptions) -> Result<()> {
    let closure_diff = adapters::NixClosureDiffAdapter;
    let brew_diff = adapters::BrewTapDiffAdapter::new(options.brew_diff);
    let notes = adapters::ReleaseNotesAdapter::new(options.notes_base);
    let extract = adapters::GithubModelsExtractAdapter;
    let store = adapters::TomlHistoryStoreAdapter::new(options.out);

    let command = RecordCommand {
        old_closure: options.old,
        new_closure: options.new,
        old_rev: options.old_rev,
        new_rev: options.new_rev,
        nixpkgs_old: options.nixpkgs_old,
        nixpkgs_new: options.nixpkgs_new,
        reference: options.reference,
        at: options.at,
    };
    application::run_record::run_record(
        command,
        &closure_diff,
        &brew_diff,
        &notes,
        &extract,
        &store,
    )
}

/// show 経路の composition root: 履歴 source を解決し adapter を結線して show use case を駆動する。
fn run_show(options: ShowOptions) -> Result<()> {
    let source = resolve_show_source(options.source)?;
    let store = adapters::TomlHistoryStoreAdapter::new(source);
    let report = adapters::StdoutHistoryReportAdapter;

    let command = ShowCommand {
        rev: options.rev,
        limit: options.limit,
        json: options.json,
        all: options.all,
    };
    application::run_show::run_show(command, &store, &report)
}

/// auto 適用後の要約を、適用前 rev からの catch-up 区間を集約して任意 sink へ描画する composition root。
///
/// flat `update` module（auto 経路）から呼ぶ再利用入口。`source` は適用済み pin 由来の
/// `docs/update-history` directory（または単一 TOML ファイル）、`applied_from_rev` は適用前の
/// `last-applied-rev`（その rev を `nixpkgs_old` に持つエントリ以降を catch-up 区間とする。`None` なら全件）。
/// `sink` には tty 時は stdout、非 tty 時は `pending-summary` ファイルなど呼び出し側が選んだ writer を渡す。
///
/// 集約・severity 再算出・重要度連動描画は show 経路（`run_show` + 共有 render）をそのまま再利用し、
/// 業務規則（catch-up 集約・severity）や表示形式を auto 経路側へ二重実装しない。`json`/`all` は固定で
/// text・宣言アプリ中心（適用後の利用者向け表示要件）。
pub(crate) fn render_applied_summary<W: Write>(
    source: &Path,
    applied_from_rev: Option<&str>,
    sink: W,
) -> Result<()> {
    let store = adapters::TomlHistoryStoreAdapter::new(source);
    let report = adapters::WriterHistoryReportAdapter::new(sink);
    let command = ShowCommand {
        rev: applied_from_rev.map(str::to_string),
        limit: None,
        json: false,
        all: false,
    };
    application::run_show::run_show(command, &store, &report)
}

/// show が読む履歴 source パスを解決する。
///
/// `--source` 明示時はその path（ファイル/ディレクトリのいずれでも可）をそのまま使う。省略時は
/// `<config-dir>/docs/update-history` **ディレクトリ**を返す（適用済み pin 由来の dotfiles input source 内）。
/// 当月ファイルを 1 本に絞り込むのではなくディレクトリを返す理由は、時刻クレートを足さず「当月」を決定
/// しないためで、ディレクトリ配下の全 `*.toml` 月次ファイルの連結読み込みは adapter
/// （[`adapters::TomlHistoryStoreAdapter`]）が名前順に行う。特定ファイルへ絞りたい場合は `--source` で
/// ファイル粒度を明示する。
fn resolve_show_source(source: Option<PathBuf>) -> Result<PathBuf> {
    match source {
        Some(source) => Ok(source),
        None => {
            let config_dir = crate::environment::config_dir(None)?;
            Ok(config_dir.join(HISTORY_SUBDIR))
        }
    }
}

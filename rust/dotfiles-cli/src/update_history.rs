//! `dotfiles update-history` の単純版（フラット）モジュール。
//!
//! nightly bump で更新されたアプリの version 差分と「何が変わったか」の構造化変更リストを
//! `docs/update-history/<YYYY-MM>.toml` に記録（`record`）し、適用済み pin の記録を閲覧（`show`）する。LLM は
//! OpenAI API（env `OPEN_AI_API_KEY`）で駆動し、1 回の record で全変更パッケージを要約しきる。取れない概要は
//! version-only（version old→new + notes_url のみ）としてその場で確定記録する（夜をまたいで再試行しない）。
//!
//! 構成はフラットな少数モジュール + 普通の関数。HTTP/LLM の差し替え点は [`llm::ChangeExtractor`] trait の 1 つ
//! だけで、決定論テストはこれを fake に差し替える。hex 階層（domain/ports/adapters/application/support の層分割・
//! port trait・mockall）は持たない。

mod diff;
mod llm;
mod notes;
mod record;
mod show;
mod wire;

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use crate::Result;

pub(crate) use show::render_applied_summary;

#[derive(Args)]
/// 更新履歴の記録（CI）・閲覧（利用者）を分けて公開する最上位 command。
pub(crate) struct UpdateHistoryOptions {
    #[command(subcommand)]
    command: UpdateHistoryCommand,
}

#[derive(Subcommand)]
/// CI が叩く記録 command と、利用者が叩く閲覧 command。
enum UpdateHistoryCommand {
    // record option は閲覧より大幅にフィールドが多いため Box で間接化して large_enum_variant を避ける。
    Record(Box<RecordOptions>),
    Show(ShowOptions),
}

#[derive(Args)]
/// nightly bump で更新されたアプリの version + 概要を 1 エントリ記録する option（CI が叩く）。
struct RecordOptions {
    /// bump 前 lock で eval した宣言パッケージの name→属性 JSON ファイル（旧 `--old` も別名で受ける）。
    #[arg(long, alias = "old")]
    nix_old: Option<PathBuf>,
    /// bump 後 lock で eval した宣言パッケージの name→属性 JSON ファイル（旧 `--new` も別名で受ける）。
    #[arg(long, alias = "new")]
    nix_new: Option<PathBuf>,
    /// brew 版差分の diff 元 rev（座標。現行 file ベース brew では未参照だが互換のため受ける）。
    #[arg(long)]
    old_rev: String,
    /// brew 版差分の diff 先 rev（座標。`--old-rev` と同様に未参照）。
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
    /// 適用時刻（RFC3339。CI が `date -u +%FT%TZ` を注入する）。
    #[arg(long)]
    at: String,
    /// 追記先の月次 TOML ファイル（`docs/update-history/<YYYY-MM>.toml`）。
    #[arg(long)]
    out: PathBuf,
    /// ノート取得元レジストリ TOML（未指定なら `--out` と同じ directory の `notes-sources.toml`）。
    #[arg(long)]
    notes_sources: Option<PathBuf>,
    /// CI が old/new tap rev から事前算出した brew 版差分ファイル（`name<TAB>old<TAB>new`）。
    #[arg(long)]
    brew_diff: Option<PathBuf>,
    /// brew cask のリリースノート URL 基底（`Casks/` レイアウト。旧 `--notes-base` も別名で受ける）。
    #[arg(long, alias = "notes-base")]
    brew_notes_base: Option<String>,
}

#[derive(Args)]
/// 適用済み pin 由来の更新履歴を閲覧する option（利用者が叩く）。
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
    /// 履歴を読む対象 source（ファイル/ディレクトリ）。省略時は state dir のローカル履歴複製。
    #[arg(long)]
    source: Option<PathBuf>,
}

/// `update-history` サブコマンドを受けて record / show を駆動する。
pub(crate) fn run(options: UpdateHistoryOptions) -> Result<()> {
    match options.command {
        UpdateHistoryCommand::Record(options) => run_record(*options),
        UpdateHistoryCommand::Show(options) => run_show(options),
    }
}

/// `--out` と同じ directory にレジストリ `notes-sources.toml` を置く既定パスを返す。
fn default_registry_path(out: &Path) -> PathBuf {
    match out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        Some(parent) => parent.join("notes-sources.toml"),
        None => PathBuf::from("notes-sources.toml"),
    }
}

fn run_record(options: RecordOptions) -> Result<()> {
    let registry_path = options
        .notes_sources
        .clone()
        .unwrap_or_else(|| default_registry_path(&options.out));
    let extractor = llm::OpenAiExtractor::new(options.brew_notes_base.clone());
    let input = record::RecordInput {
        nixpkgs_old: options.nixpkgs_old,
        nixpkgs_new: options.nixpkgs_new,
        reference: options.reference,
        at: options.at,
        out: &options.out,
        registry_path: &registry_path,
        nix_old: options.nix_old.as_deref(),
        nix_new: options.nix_new.as_deref(),
        brew_diff: options.brew_diff.as_deref(),
    };
    record::run_record(input, &extractor)
}

fn run_show(options: ShowOptions) -> Result<()> {
    let source = match options.source {
        Some(source) => source,
        None => crate::update::history_local_dir()?,
    };
    show::run_show(
        &source,
        options.rev.as_deref(),
        options.limit,
        options.json,
        options.all,
    )
}

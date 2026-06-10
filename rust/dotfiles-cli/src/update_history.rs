//! `dotfiles update-history` モジュール。
//!
//! nightly bump で更新されたアプリの version 差分と「何が変わったか」の構造化変更リストを
//! `docs/update-history/<YYYY-MM>.toml` に記録（`record`）し、適用済み pin の記録を閲覧（`show`）する。LLM は
//! OpenAI API（env `OPEN_AI_API_KEY`）で駆動し、1 回の record で全変更パッケージを要約しきる。概要が取れない
//! パッケージは version-only（version old→new + notes_url のみ）としてその場で確定記録する。
//!
//! nightly パイプラインの**ロジックは全て Rust に集約**する。`record` は ci-ref と old/new lock を受けて
//! eval（[`eval`]: `nix eval` 起動 + owner/repo 導出）→ nix 版差分 → brew cask 版差分（[`brew`]: reqwest で
//! cask `.rb` 取得・version 解析）→ ノート取得 → OpenAI 要約 → TOML 記録までを完結する。bash/nix/yaml には
//! 避けられない nix tool 呼び出し（`nix eval` / `nix flake update`）以外のロジックを置かない。`eval-versions` /
//! `lock-rev` は workflow が old を bump 前に評価するための薄い Rust ラッパで、整形・導出・rev 抽出は Rust が担う。
//!
//! 構成はフラットな少数モジュール + 普通の関数。HTTP/LLM の差し替え点は [`llm::ChangeExtractor`] trait の 1 つ
//! だけで、決定論テストはこれを fake に差し替える。

mod brew;
mod diff;
mod eval;
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
/// CI が叩く記録・eval・rev 抽出 command と、利用者が叩く閲覧 command。
enum UpdateHistoryCommand {
    // record option は他より大幅にフィールドが多いため Box で間接化して large_enum_variant を避ける。
    Record(Box<RecordOptions>),
    EvalVersions(EvalVersionsOptions),
    LockRev(LockRevOptions),
    Show(ShowOptions),
}

#[derive(Args)]
/// nightly bump で更新されたアプリの version + 概要を 1 エントリ記録する option（CI が叩く）。
struct RecordOptions {
    /// bump 前 lock で eval した宣言パッケージの name→属性 JSON ファイル（`eval-versions` が bump 前に書く）。
    #[arg(long, alias = "old")]
    nix_old: Option<PathBuf>,
    /// bump 後 lock の宣言パッケージ JSON（省略時は `--reference` を `nix eval` して Rust で導出する）。
    #[arg(long, alias = "new")]
    nix_new: Option<PathBuf>,
    /// 記録する bump 前 nixpkgs リビジョン。
    #[arg(long)]
    nixpkgs_old: String,
    /// 記録する bump 後 nixpkgs リビジョン。
    #[arg(long)]
    nixpkgs_new: String,
    /// diff 対象の参照構成（例: `darwinConfigurations.<ref>`）。`--nix-new` 省略時は eval にも使う。
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
    /// 宣言 cask を読む `homebrew.nix` path（指定時、両 cask rev から版差分を Rust で算出する）。
    #[arg(long)]
    homebrew_nix: Option<PathBuf>,
    /// brew cask tap の bump 前 rev（`--homebrew-nix` と対で cask 版差分に使う）。
    #[arg(long)]
    cask_rev_old: Option<String>,
    /// brew cask tap の bump 後 rev（cask 版差分と、cask 探索ヒントの `Casks/` 基底に使う）。
    #[arg(long)]
    cask_rev_new: Option<String>,
}

#[derive(Args)]
/// 参照構成の宣言パッケージ版を `nix eval` し、導出済み name→属性 JSON を書く option（bump 前後で叩く）。
struct EvalVersionsOptions {
    /// eval する参照構成（例: `darwinConfigurations.ci-ref`）。
    #[arg(long)]
    reference: String,
    /// 書き出す JSON ファイル path。
    #[arg(long)]
    out: PathBuf,
}

#[derive(Args)]
/// flake.lock の `nodes.<node>.locked.rev` を取り出して標準出力へ書く option。
struct LockRevOptions {
    /// 読む flake.lock path。
    #[arg(long)]
    lock: PathBuf,
    /// rev を取り出すノード名（例: `nixpkgs` / `homebrew-homebrew-cask`）。
    #[arg(long)]
    node: String,
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

/// `update-history` サブコマンドを受けて record / eval-versions / lock-rev / show を駆動する。
pub(crate) fn run(options: UpdateHistoryOptions) -> Result<()> {
    match options.command {
        UpdateHistoryCommand::Record(options) => run_record(*options),
        UpdateHistoryCommand::EvalVersions(options) => run_eval_versions(options),
        UpdateHistoryCommand::LockRev(options) => run_lock_rev(options),
        UpdateHistoryCommand::Show(options) => run_show(options),
    }
}

/// 参照構成を `nix eval` し、導出済み宣言パッケージ JSON を `--out` へ書く（bump 前後で叩く）。
fn run_eval_versions(options: EvalVersionsOptions) -> Result<()> {
    let versions = eval::eval_declared_versions(&options.reference)?;
    if let Some(parent) = options.out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&options.out, serde_json::to_string(&versions)?)?;
    Ok(())
}

/// flake.lock のノード rev を標準出力へ書く（不在は空行）。
fn run_lock_rev(options: LockRevOptions) -> Result<()> {
    let rev = eval::lock_node_rev(&options.lock, &options.node)?.unwrap_or_default();
    println!("{rev}");
    Ok(())
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
    // cask 探索ヒントの `Casks/` 基底は bump 後 cask rev から組み立てる（無ければ brew は探索ヒント無し）。
    let brew_notes_base = options.cask_rev_new.as_deref().map(|rev| {
        format!("https://raw.githubusercontent.com/homebrew/homebrew-cask/{rev}/Casks/")
    });
    let extractor = llm::OpenAiExtractor::new(brew_notes_base);
    let input = record::RecordInput {
        nixpkgs_old: options.nixpkgs_old,
        nixpkgs_new: options.nixpkgs_new,
        reference: options.reference,
        at: options.at,
        out: &options.out,
        registry_path: &registry_path,
        nix_old: options.nix_old.as_deref(),
        nix_new: options.nix_new.as_deref(),
        homebrew_nix: options.homebrew_nix.as_deref(),
        cask_rev_old: options.cask_rev_old.as_deref(),
        cask_rev_new: options.cask_rev_new.as_deref(),
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

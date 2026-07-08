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
mod sources;
mod wire;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use crate::{Result, local_flake, process::run_capture};

/// 適用済み dotfiles flake input source 内の更新履歴ディレクトリ（`<source>/docs/update-history`）。
const HISTORY_SUBDIR: &str = "docs/update-history";

#[derive(Args)]
/// 更新履歴の記録（CI）・閲覧（利用者）を分けて公開する最上位 command。
pub(crate) struct UpdateHistoryOptions {
    #[command(subcommand)]
    command: UpdateHistoryCommand,
}

#[derive(Subcommand)]
/// CI 用の記録・eval・rev 抽出 command と、利用者向けの閲覧・version-only backfill command。
enum UpdateHistoryCommand {
    // record option は他より大幅にフィールドが多いため Box で間接化して large_enum_variant を避ける。
    Record(Box<RecordOptions>),
    BackfillVersionOnly(BackfillVersionOnlyOptions),
    EvalVersions(EvalVersionsOptions),
    LockRev(LockRevOptions),
    Show(ShowOptions),
}

#[derive(Args)]
/// nightly bump で更新されたアプリの version + 概要を 1 エントリ記録する option（CI が叩く）。
struct RecordOptions {
    /// bump 前 lock ファイル（state key は Rust 側で算出する）。
    #[arg(long, requires = "lock_new")]
    lock_old: Option<PathBuf>,
    /// bump 後 lock ファイル（state key は Rust 側で算出する）。
    #[arg(long, requires = "lock_old")]
    lock_new: Option<PathBuf>,
    /// 既存 `show --rev` 利用者向けの legacy cursor old（通常は bump 前 repo HEAD）。
    #[arg(long)]
    cursor_old: Option<String>,
    /// 必要なら保持する legacy cursor new。
    #[arg(long)]
    cursor_new: Option<String>,
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
/// 既存の月次履歴 TOML に残っている version-only package を、現在の取得ロジックで再処理して埋め直す。
struct BackfillVersionOnlyOptions {
    /// 更新対象の月次履歴 TOML。
    #[arg(long)]
    history: PathBuf,
    /// provenance レジストリ TOML（未指定なら `--history` と同じ directory の `notes-sources.toml`）。
    #[arg(long)]
    notes_sources: Option<PathBuf>,
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
    /// 表示起点の状態キー（新形式は `state_old`、旧履歴は `cursor_old`/`nixpkgs_old`。互換 alias: `--rev`）。
    #[arg(long, alias = "rev")]
    state: Option<String>,
    /// 表示するエントリ件数の上限。
    #[arg(long)]
    limit: Option<usize>,
    /// 生データ（JSON）で出力する。
    #[arg(long)]
    json: bool,
    /// 宣言アプリだけでなく全パッケージを表示する。
    #[arg(long)]
    all: bool,
    /// 履歴を読む対象 source（ファイル/ディレクトリ）。省略時は適用済み dotfiles input source の更新履歴 dir。
    #[arg(long)]
    source: Option<PathBuf>,
    /// 省略 source 解決に使うローカル flake の設定 dir（省略時は `$HOME/.config/dotfiles`）。
    #[arg(long, env = "DOTFILES_CONFIG_DIR", value_name = "PATH")]
    config_dir: Option<PathBuf>,
}

/// `update-history` サブコマンドを受けて record / backfill-version-only /
/// eval-versions / lock-rev / show を駆動する。
pub(crate) fn run(options: UpdateHistoryOptions) -> Result<()> {
    match options.command {
        UpdateHistoryCommand::Record(options) => run_record(*options),
        UpdateHistoryCommand::BackfillVersionOnly(options) => run_backfill_version_only(options),
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
        lock_old: options.lock_old.as_deref(),
        lock_new: options.lock_new.as_deref(),
        cursor_old: options.cursor_old,
        cursor_new: options.cursor_new,
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

fn run_backfill_version_only(options: BackfillVersionOnlyOptions) -> Result<()> {
    let registry_path = options
        .notes_sources
        .clone()
        .unwrap_or_else(|| default_registry_path(&options.history));
    let extractor = llm::OpenAiExtractor::new(None);
    record::run_backfill_version_only(&options.history, &registry_path, &extractor)
}

/// 利用者 `show`: 履歴 source を読み、起点 state（旧履歴は `cursor_old`/`nixpkgs_old` fallback）からの catch-up 区間を
/// 集約して stdout へ出力する。
///
/// `--source` 省略時は適用済み dotfiles input source の更新履歴 dir を解決する（`update` と同じ stateless 経路。
/// 永続 state を参照しない）。source を解決できない（network 無し・nix 不在・archive 失敗等）場合は `Err` で止める。
fn run_show(options: ShowOptions) -> Result<()> {
    let source = match options.source {
        Some(source) => source,
        None => {
            let config_dir = crate::environment::config_dir(options.config_dir)?;
            resolve_history_source(&config_dir).ok_or_else(|| {
                anyhow::anyhow!(
                    "適用済み dotfiles input source の更新履歴を解決できませんでした\
                     （`--source` で履歴 dir を明示してください）"
                )
            })?
        }
    };
    show::run_show(
        &source,
        options.state.as_deref(),
        options.limit,
        options.json,
        options.all,
    )
}

/// 適用済み dotfiles input source の `docs/update-history` dir を解決する（解決不能なら `None`）。
///
/// `show`（`--source` 未指定時）の既定 source。適用済み dotfiles flake input が指す realize 済み store path
/// （`docs/update-history`）から offline・決定論で読み、永続 state を参照しない。
fn resolve_history_source(config_dir: &Path) -> Option<PathBuf> {
    resolve_input_source(config_dir).map(|source| source.join(HISTORY_SUBDIR))
}

/// 適用済み dotfiles flake input の **realize 済み source store path** を解決する（解決不能なら `None`）。
///
/// `nix flake archive <config-dir> --json --no-write-lock-file` の `inputs.<dotfiles>.path` を返す。metadata の
/// `locked` でなく archive を使うのは、本番の github 型 input が metadata に `path` を持たないためである。
/// network 無し・nix 不在・archive 失敗・JSON 解析失敗はいずれも `None` へ縮退する（履歴解決は best-effort）。
fn resolve_input_source(config_dir: &Path) -> Option<PathBuf> {
    let args = [
        OsString::from("flake"),
        OsString::from("archive"),
        config_dir.as_os_str().to_os_string(),
        OsString::from("--json"),
        OsString::from("--no-write-lock-file"),
    ];
    let json = run_capture("nix", args).ok()?;
    parse_input_source_path(&json, local_flake::INPUT_NAME).map(PathBuf::from)
}

/// `nix flake archive --json` 出力から指定 input の realize 済み source store path を抽出する純粋関数。
fn parse_input_source_path(archive_json: &str, input_name: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(archive_json).ok()?;
    value
        .get("inputs")
        .and_then(|inputs| inputs.get(input_name))
        .and_then(|node| node.get("path"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn record_requires_both_lock_paths() -> crate::Result<()> {
        let parsed = crate::cli::Cli::try_parse_from([
            "dotfiles",
            "update-history",
            "record",
            "--lock-old",
            "old.lock",
            "--nixpkgs-old",
            "old",
            "--nixpkgs-new",
            "new",
            "--reference",
            "darwinConfigurations.ci",
            "--at",
            "2026-06-05T18:00:11Z",
            "--out",
            "2026-06.toml",
        ]);
        assert!(
            parsed.is_err(),
            "missing paired --lock-new must be rejected"
        );
        let err = parsed
            .err()
            .ok_or_else(|| anyhow::anyhow!("asserted above: parse must fail"))?;
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        Ok(())
    }
}

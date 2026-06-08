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
    mod diagnostics;
    mod github_models;
    mod nix;
    mod notes;
    mod registry_store;
    mod report;
    mod toml_store;

    pub(in crate::update_history) use brew::BrewTapDiffAdapter;
    pub(in crate::update_history) use diagnostics::StderrRecordDiagnosticsAdapter;
    pub(in crate::update_history) use github_models::GithubModelsExtractAdapter;
    pub(in crate::update_history) use nix::NixEvalVersionAdapter;
    pub(in crate::update_history) use notes::ReleaseNotesAdapter;
    pub(in crate::update_history) use registry_store::TomlNotesSourceRegistryAdapter;
    pub(in crate::update_history) use report::{
        StdoutHistoryReportAdapter, WriterHistoryReportAdapter,
    };
    pub(in crate::update_history) use toml_store::TomlHistoryStoreAdapter;
}
mod application;
pub(crate) mod domain;
pub(crate) mod ports;
/// process-generic な安全 fetch primitive（業務語彙を持たない技術境界。notes / github_models 双方が再利用）。
mod support;

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use crate::Result;
use domain::commands::{RecordCommand, ShowCommand};

#[derive(Args)]
/// 更新履歴の記録（CI）と閲覧（利用者）を分けて公開する最上位 command。
pub(crate) struct UpdateHistoryOptions {
    #[command(subcommand)]
    command: UpdateHistoryCommand,
}

#[derive(Subcommand)]
/// CI が叩く記録 command と、利用者が叩く閲覧 command。
enum UpdateHistoryCommand {
    // record option は閲覧 option より大幅にフィールドが多く enum variant 間で size 差が出るため、
    // `Box` で間接化して `large_enum_variant` を避ける（clap は `Box<Args>` を subcommand 変種に取れる）。
    Record(Box<RecordOptions>),
    Show(ShowOptions),
}

#[derive(Args)]
/// nightly bump で更新されたアプリの version + 概要を 1 エントリ記録する option。
///
/// CI（network + GitHub Models）が叩く。nix 版差分は eval ベース: CI が ci-ref の old/new lock で
/// `nix eval --json` した宣言パッケージの name→version JSON ファイル（`--nix-old`/`--nix-new`）を読み、
/// domain の純粋比較で差分を求める（フル closure を `diff-closures` で 2 回ビルドする必要はない）。brew 版差分は
/// `--brew-diff` ファイルから読む。各アプリの生ノートを取得して LLM で構造化抽出し、`--out` の月次 TOML へ
/// 追記する。`--at` は RFC3339 を注入する。
struct RecordOptions {
    /// bump 前 lock で eval した宣言パッケージの name→属性 JSON ファイル
    /// （`{ "name": { "version": "...", "repo": "owner/repo", "changelog": "..." }, ... }`。`repo`/`changelog`
    /// は省略可で `serde(default)`、旧 `notes_source` key も `changelog` の alias で受ける）。未指定なら nix old
    /// 側は空マップへ縮退する。後方互換のため旧 `--old` も別名として受ける。
    #[arg(long, alias = "old")]
    nix_old: Option<PathBuf>,
    /// bump 後 lock で eval した宣言パッケージの name→version JSON ファイル。未指定なら nix new 側は空マップ
    /// へ縮退する。後方互換のため旧 `--new` も別名として受ける。
    #[arg(long, alias = "new")]
    nix_new: Option<PathBuf>,
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
    /// ノート取得元レジストリ（provenance の学習・再利用）の TOML ファイル
    /// （`docs/update-history/notes-sources.toml`）。利用者要件 (3)/(4): 取得元をここへ保存し、次回 record は
    /// これを最優先参照して再利用し再探索しない（AI 探索を新規/未知/自己修復のみへ限定してレートを逓減）。
    /// 未指定なら `--out` と同じ directory の `notes-sources.toml` を既定にする（nightly が commit する
    /// `docs/update-history/**` 内に収まり、レジストリも同経路で repo に入る）。
    #[arg(long)]
    notes_sources: Option<PathBuf>,
    /// CI が old/new tap rev から事前算出した brew 版差分ファイル（`name<TAB>old<TAB>new`）。
    /// 未指定なら brew 差分は縮退して空。
    #[arg(long)]
    brew_diff: Option<PathBuf>,
    /// brew cask のリリースノート URL 基底（cask 定義の `Casks/` レイアウト。`<base><letter>/<name>.rb` を
    /// 取得対象にする）。brew tap 由来 package にだけ使う。未指定なら brew package のノート取得は縮退して空。
    /// 後方互換のため旧 `--notes-base` も別名として受ける（旧運用は cask base を渡していた）。
    /// nix eval 由来 package のノート取得先は `--nix-old`/`--nix-new` の eval JSON が各パッケージごとに
    /// 運ぶ `notes_source`（`meta.changelog`/`meta.homepage`）を使うため、nix 用の base 引数は持たない。
    #[arg(long, alias = "notes-base")]
    brew_notes_base: Option<String>,
}

#[derive(Args)]
/// 適用済み pin 由来の更新履歴を閲覧する option。
///
/// `--rev` 起点からの catch-up 区間をアプリ単位で集約し、severity バッジ + 全体概要 + アプリ別変更リストを
/// 表示する。`--source` 省略時は state dir のローカル履歴複製（`<state-dir>/history`）を読む。これは
/// `dotfiles update` 適用時に dotfiles input source の `docs/update-history` から複製されたものである。
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
    /// 履歴を読む対象 source（ファイル/ディレクトリ）。省略時は state dir のローカル履歴複製
    /// （`<state-dir>/history`）ディレクトリを既定 source とし、配下の全 `*.toml` 月次ファイルを連結して読む。
    #[arg(long)]
    source: Option<PathBuf>,
}

/// CLI で parse 済みの `dotfiles update-history` command を composition root へ渡す。
///
/// CLI 入口は command 定義と option 変換だけを担い、adapter concrete 生成と use case 結線は
/// composition root（[`run_record`] / [`run_show`]）へ閉じる。
pub(crate) fn run(options: UpdateHistoryOptions) -> Result<()> {
    match options.command {
        UpdateHistoryCommand::Record(options) => run_record(*options),
        UpdateHistoryCommand::Show(options) => run_show(options),
    }
}

/// record 経路の composition root: adapter concrete を結線し record use case を駆動する。
fn run_record(options: RecordOptions) -> Result<()> {
    // レジストリ path: 明示指定が無ければ `--out` と同じ directory の `notes-sources.toml` を既定にする
    // （nightly が commit する `docs/update-history/**` 内に収め、レジストリも同経路で repo に入れる）。
    let registry_path = options
        .notes_sources
        .unwrap_or_else(|| default_registry_path(&options.out));

    let nix_versions = adapters::NixEvalVersionAdapter::new(options.nix_old, options.nix_new);
    let brew_diff = adapters::BrewTapDiffAdapter::new(options.brew_diff);
    let notes = adapters::ReleaseNotesAdapter::new(options.brew_notes_base);
    let extract = adapters::GithubModelsExtractAdapter::new();
    let store = adapters::TomlHistoryStoreAdapter::new(options.out);
    let registry_store = adapters::TomlNotesSourceRegistryAdapter::new(registry_path);
    // 縮退・provenance 経路の診断は adapter（stderr 出力）へ閉じ、application から concrete I/O を排除する。
    let diagnostics = adapters::StderrRecordDiagnosticsAdapter;

    let command = RecordCommand {
        old_rev: options.old_rev,
        new_rev: options.new_rev,
        nixpkgs_old: options.nixpkgs_old,
        nixpkgs_new: options.nixpkgs_new,
        reference: options.reference,
        at: options.at,
    };
    let runtime = application::run_record::RecordRuntime {
        nix_versions: &nix_versions,
        brew_diff: &brew_diff,
        notes: &notes,
        extract: &extract,
        store: &store,
        registry_store: &registry_store,
        diagnostics: &diagnostics,
    };
    application::run_record::run_record(command, &runtime)
}

/// `--out`（月次 TOML）の置き場と同じ directory にレジストリ `notes-sources.toml` を置く既定パスを返す。
///
/// レジストリは月次履歴と同じ `docs/update-history/` 配下に置き、nightly の commit 許可パス
/// （`docs/update-history/**`）内に収める。`--out` に親 directory が無い（ファイル名のみ）場合は
/// カレント directory の `notes-sources.toml` にする。
fn default_registry_path(out: &Path) -> PathBuf {
    match out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        Some(parent) => parent.join("notes-sources.toml"),
        None => PathBuf::from("notes-sources.toml"),
    }
}

/// show 経路の composition root: 履歴 source を解決し adapter を結線して show use case を駆動する。
fn run_show(options: ShowOptions) -> Result<()> {
    let source = resolve_show_source(options.source)?;
    let store = adapters::TomlHistoryStoreAdapter::new(source);
    let report = adapters::StdoutHistoryReportAdapter;

    let command = ShowCommand {
        rev: options.rev,
        // 利用者 `show` は nixpkgs rev 起点（`--rev`）で選ぶ。`after_at` は適用後要約専用カーソルのため None。
        after_at: None,
        limit: options.limit,
        json: options.json,
        all: options.all,
        // 利用者 `show` は全出所を表示する（適用後要約の target 絞り込みは update 経路専用）。
        source_filter: domain::wire::PackageSourceFilter::All,
    };
    application::run_show::run_show(command, &store, &report)
}

/// auto 適用後の要約を、要約済み `at` カーソル以降の catch-up 区間を集約して任意 sink へ描画する
/// composition root。要約し終えた終端エントリの `at`（次回カーソル）を返す。
///
/// flat `update` module（auto 経路）から呼ぶ再利用入口。`source` は適用済み pin 由来の
/// `docs/update-history` directory（または単一 TOML ファイル）、`summarized_after_at` は**前回要約し終えた
/// エントリの `at`**（その `at` より後に記録されたエントリだけを catch-up 区間とする。`None` なら全件 = 初回）。
/// `sink` には tty 時は stdout、非 tty 時は `pending-summary` ファイルなど呼び出し側が選んだ writer を渡す。
/// `source_filter` は実際に適用した target に対応する出所だけへ要約を絞る（finding 3368653947。home 部分適用は
/// `NixOnly` で brew cask を除外して未適用 cask を通知しない。全体/darwin 適用は `All`）。
///
/// **nixpkgs rev ではなく `at` カーソルを使う理由**: brew tap だけが進み `nixpkgs_old == nixpkgs_new`
/// （= 同一 nixpkgs rev）の brew-only 更新が複数できると、nixpkgs rev 起点では `N -> N` を越えて進めず、
/// 要約済みの brew-only 更新を毎回再表示してしまう。各エントリの `at` は記録のたびに前進する一意値なので、
/// 要約済み `at` を単調カーソルにすれば一度要約した更新を再表示しない（[`select_entries_after`]）。
///
/// 戻り値は要約し終えた終端エントリの `at`。呼び出し側はこれを要約済み marker（`at` カーソル）へ書き、次回の
/// `summarized_after_at` に渡す。選択範囲が空（新規更新なし）なら `None` を返し、marker は進めない。
/// 集約・severity 再算出・重要度連動描画は show 経路（`run_applied_summary` + 共有 helper）を再利用し、
/// 業務規則や表示形式を二重実装しない。`json`/`all` は固定で text・宣言アプリ中心（適用後の利用者向け表示要件）。
///
/// caller responsibility: 呼び出し側（`update` module）は要約済み marker を nixpkgs rev ではなく
/// **`at` 値**で保持し、その値を `summarized_after_at` へ渡し、戻り値（次回カーソル）を marker へ確定する。
///
/// [`select_entries_after`]: crate::update_history::domain::selection::select_entries_after
pub(crate) fn render_applied_summary<W: Write>(
    source: &Path,
    summarized_after_at: Option<&str>,
    source_filter: domain::wire::PackageSourceFilter,
    sink: W,
) -> Result<Option<String>> {
    let store = adapters::TomlHistoryStoreAdapter::new(source);
    let report = adapters::WriterHistoryReportAdapter::new(sink);
    let command = ShowCommand {
        rev: None,
        after_at: summarized_after_at.map(str::to_string),
        limit: None,
        json: false,
        all: false,
        // 適用後要約は実際に適用した target に対応する出所だけへ絞る（home 部分適用は nix のみ）。
        source_filter,
    };
    application::run_show::run_applied_summary(command, &store, &report)
}

/// show が読む履歴 source パスを解決する。
///
/// `--source` 明示時はその path（ファイル/ディレクトリのいずれでも可）をそのまま使う。省略時は **state dir の
/// ローカル複製**（`<state-dir>/history`、[`crate::update::history_local_dir`]）を返す。`~/.config/dotfiles` の
/// ローカル flake は `flake.nix`/`flake.lock` だけを持ち `docs/update-history` を含まないため、config dir を
/// 直接読むと show が常に空になる。履歴は **適用済み dotfiles input source** にあり、`dotfiles update` が適用時
/// にそれを state dir のローカル複製へ取り込む。show はその複製を offline・決定論で読む。ディレクトリ配下の全
/// `*.toml` 月次ファイルの連結読み込みは adapter（[`adapters::TomlHistoryStoreAdapter`]）が名前順に行う。
/// 特定ファイルへ絞りたい場合は `--source` でファイル粒度を明示する。
fn resolve_show_source(source: Option<PathBuf>) -> Result<PathBuf> {
    match source {
        Some(source) => Ok(source),
        None => crate::update::history_local_dir(),
    }
}

#[cfg(test)]
mod tests {
    //! composition root の `render_applied_summary` が `at` カーソルで要約し終えた終端 `at` を返し、
    //! brew-only `N -> N` 更新を再表示しないことを、実 TOML 履歴を読んで end-to-end で固定する。

    use std::io::Write as _;

    use super::domain::wire::PackageSourceFilter;
    use super::render_applied_summary;
    use crate::Result;

    /// 一時 dir に月次 TOML を 1 ファイル書き、その dir を source として返す（adapter は dir 配下を連結読みする）。
    fn write_history_dir(name: &str, toml: &str) -> Result<std::path::PathBuf> {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "dotfiles-render-applied-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let mut file = std::fs::File::create(dir.join("2026-06.toml"))?;
        file.write_all(toml.as_bytes())?;
        Ok(dir)
    }

    #[test]
    fn render_applied_summary_uses_at_cursor_and_returns_terminal_at() -> Result<()> {
        // brew-only 2 夜（nixpkgs_old==nixpkgs_new="N"）。`at` カーソルで一度要約したら再表示しない。
        let toml = "\
[[update]]
at = \"2026-06-01T00:00:00Z\"
nixpkgs_old = \"N\"
nixpkgs_new = \"N\"
reference = \"darwinConfigurations.ci\"
severity = \"minor\"
overall = \"1アプリ更新: ✨1\"

[[update.package]]
name = \"firefox\"
old = \"120\"
new = \"121\"
change = \"upgraded\"
declared = true
source = \"brew\"

[[update.package.change_item]]
category = \"feature\"
text = \"新機能\"

[[update]]
at = \"2026-06-02T00:00:00Z\"
nixpkgs_old = \"N\"
nixpkgs_new = \"N\"
reference = \"darwinConfigurations.ci\"
severity = \"minor\"
overall = \"1アプリ更新: 🐛1\"

[[update.package]]
name = \"slack\"
old = \"4.0\"
new = \"4.1\"
change = \"upgraded\"
declared = true
source = \"brew\"

[[update.package.change_item]]
category = \"fix\"
text = \"修正\"
";
        let dir = write_history_dir("at-cursor", toml)?;

        // 初回（marker 無し）: 全 brew-only 更新を要約し、終端 `at` を返す。
        let mut buf1: Vec<u8> = Vec::new();
        let cursor = render_applied_summary(&dir, None, PackageSourceFilter::All, &mut buf1)?;
        let rendered1 = String::from_utf8(buf1)?;
        assert!(rendered1.contains("firefox"), "{rendered1:?}");
        assert!(rendered1.contains("slack"), "{rendered1:?}");
        assert_eq!(cursor.as_deref(), Some("2026-06-02T00:00:00Z"));

        // 2 回目（marker = 終端 at）: 新規が無いので brew-only 更新を再表示しない（見出しのみ「0アプリ更新」）。
        let mut buf2: Vec<u8> = Vec::new();
        let cursor2 =
            render_applied_summary(&dir, cursor.as_deref(), PackageSourceFilter::All, &mut buf2)?;
        let rendered2 = String::from_utf8(buf2)?;
        assert!(
            !rendered2.contains("firefox") && !rendered2.contains("slack"),
            "要約済み brew-only 更新を再表示しない: {rendered2:?}"
        );
        assert!(rendered2.contains("0アプリ更新"), "{rendered2:?}");
        assert_eq!(cursor2, None, "空 span では marker を進めない");

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn home_target_summary_excludes_brew_cask_updates() -> crate::Result<()> {
        // finding 3368653947 退行固定: home 部分適用（`NixOnly`）の要約は brew cask 更新を出さない。CI 履歴には
        // nix package（neovim）と brew cask（firefox）が両方あるが、home だけ switch した直後の要約は home-manager
        // の nix 更新だけを見せ、未適用の cask（Firefox）を通知しない。`All`（全体/darwin 適用）は両方見せる。
        let toml = "\
[[update]]
at = \"2026-06-01T00:00:00Z\"
nixpkgs_old = \"N0\"
nixpkgs_new = \"N1\"
reference = \"darwinConfigurations.ci\"
severity = \"minor\"
overall = \"2アプリ更新\"

[[update.package]]
name = \"neovim\"
old = \"0.10\"
new = \"0.11\"
change = \"upgraded\"
declared = true
source = \"nix\"

[[update.package.change_item]]
category = \"feature\"
text = \"新機能\"

[[update.package]]
name = \"firefox\"
old = \"120\"
new = \"121\"
change = \"upgraded\"
declared = true
source = \"brew\"

[[update.package.change_item]]
category = \"feature\"
text = \"cask 新機能\"
";
        let dir = write_history_dir("home-filter", toml)?;

        // home 部分適用（NixOnly）: nix の neovim だけ要約し、brew cask の firefox は出さない。
        let mut buf_home: Vec<u8> = Vec::new();
        render_applied_summary(&dir, None, PackageSourceFilter::NixOnly, &mut buf_home)?;
        let home = String::from_utf8(buf_home)?;
        assert!(
            home.contains("neovim"),
            "home 要約は nix package を出す: {home:?}"
        );
        assert!(
            !home.contains("firefox"),
            "home 部分適用は未適用の brew cask を出さない: {home:?}"
        );

        // 全体適用（All）: nix も brew cask も両方出す。
        let mut buf_all: Vec<u8> = Vec::new();
        render_applied_summary(&dir, None, PackageSourceFilter::All, &mut buf_all)?;
        let all = String::from_utf8(buf_all)?;
        assert!(all.contains("neovim"), "全体要約は nix を出す: {all:?}");
        assert!(
            all.contains("firefox"),
            "全体要約は brew cask も出す: {all:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}

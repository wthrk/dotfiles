//! `dotfiles update` の auto 適用経路。ローカル flake の repo pin を追随し、必要時だけ適用する。
//!
//! `switch` は lock 済みの入力をそのまま使う。`update` は repo pin（生成ローカル flake の `flake.lock`
//! における dotfiles input の locked rev）が前回適用済み rev と異なるときだけ、`flake update` + switch を
//! 実行して fleet を repo pin へ収束させる。scheduler（launchd daemon）とインタラクティブシェルの双方が
//! この同じ `dotfiles update` を呼ぶため、同時適用を `update.lock` の排他で防ぐ。
//!
//! 既定では dotfiles input だけを更新し、推移的 nixpkgs を dotfiles repo の committed lock に追従させる。
//! `--full` 指定時のみ input 名を渡さず、ローカル flake の全入力を最新解決へ更新する。
//!
//! ## 状態ディレクトリと所有権
//!
//! 状態は `$XDG_STATE_HOME/dotfiles`（未設定なら `$HOME/.local/state/dotfiles`）に置く。auto-update.nix の
//! ラッパーは `darwin-rebuild` を root、ユーザ状態（このバイナリ）を `sudo -u <user>` で呼ぶ前提であり、
//! 本 module が書く `last-applied-rev` / `pending-summary` / `last-run.log` / `update.lock` は**必ずユーザ所有**で
//! 作られる。root では状態ファイルを作らない設計のため、本 module は `$HOME` 配下のユーザ state dir にしか
//! 書かず、所有権昇格・root 専用パスへの書込みを行わない。
//!
//! ## 排他とアトミシティ
//!
//! `update.lock` を `O_EXCL`（`create_new`）ベースで取得し、「適用要否判定 → flake update → switch →
//! `last-applied-rev` 更新」を単一区間で連続保持して TOCTOU を避ける。pin は lock 取得後に再読する。lock を
//! 取得できなかった側（既に他プロセスが適用中）は skip（exit 0）し、次回シェル/スケジュールで再判定する。
//! `last-applied-rev` は temp ファイルへ書いてから rename する原子的書込みで、部分書込みを観測させない。
//! 適用要否は暦日でなく rev 比較で決める（1 日複数回適用可、同日重複適用は rev 一致で抑止）。

use std::ffi::OsString;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::Args;

use crate::{Result, local_flake, process::run as run_process, switch, update_history};

/// state dir 配下のファイル名（いずれもユーザ所有で作る）。
const LAST_APPLIED_REV: &str = "last-applied-rev";
const PENDING_SUMMARY: &str = "pending-summary";
const LAST_RUN_LOG: &str = "last-run.log";
const LOCK_FILE: &str = "update.lock";

/// `docs/update-history` を解決する config-dir 相対のサブディレクトリ。
const HISTORY_SUBDIR: &str = "docs/update-history";

/// auto 経路の入口。repo pin を読み、前回適用済み rev と異なるときだけ適用し、適用後要約を振り分ける。
///
/// 順序: state dir 確保 → `update.lock` 非ブロッキング取得 → （取得失敗なら skip）→ lock 保持下で pin 再読 →
/// `last-applied-rev`（dotfiles pin）と比較 → 異なれば適用前の推移的 nixpkgs old rev を解決 → `flake update`
/// と switch を実行 → `last-applied-rev` を原子的更新 → **適用前 nixpkgs rev 起点**の catch-up 要約を tty/
/// 非 tty で振り分け表示。要約選択は `nixpkgs_old` と突合するため、dedup 用 dotfiles pin ではなく nixpkgs
/// old rev を起点に渡す（名前空間が異なるため pin SHA を渡すと span が恒久空になる）。lock は処理終端で
/// 解放する（guard の drop）。`--dry-run` では実際の適用・状態書込みをせず、判定・表示経路だけを通す。
pub(crate) fn run(options: UpdateOptions) -> Result<()> {
    let config_dir = options.switch.config_dir()?;
    switch::ensure_config_exists(&config_dir)?;

    let state_dir = state_dir()?;
    let dry_run = options.switch.dry_run();
    if !dry_run {
        // 状態ファイルはユーザ所有の state dir 配下にしか作らない。root では呼ばれない前提（auto-update.nix）。
        fs::create_dir_all(&state_dir)
            .with_context(|| format!("failed to create state dir {}", state_dir.display()))?;
    }

    // lock 取得失敗 = 他プロセスが適用中。skip して次回再判定（exit 0）。
    let Some(_lock) = UpdateLock::try_acquire(&state_dir, dry_run)? else {
        println!("別の dotfiles update が適用中のため skip します");
        return Ok(());
    };

    // lock 取得後に pin を再読し、判定〜適用〜マーカー更新を単一 lock 区間で連続させる（TOCTOU 回避）。
    let current_pin = read_repo_pin(&config_dir)?;

    // `--commit-rev-marker`: 二段適用（home→darwin）の **両成功後** に rev マーカーだけを確定させる経路。
    // home/darwin が別 CLI 起動に分かれる daemon ラッパーで、darwin 成功後にここを呼び、適用済み pin を
    // 記録する。適用・要約は行わず、現在 pin を `last-applied-rev` へ原子的に書くだけ（lock 区間内で行う）。
    if options.commit_rev_marker {
        write_last_applied_rev(&state_dir, &current_pin, dry_run)?;
        println!("適用済み rev を確定しました（rev {current_pin}）");
        return Ok(());
    }

    let previous_rev = read_last_applied_rev(&state_dir)?;

    if previous_rev.as_deref() == Some(current_pin.as_str()) {
        // 現在 pin が前回適用済みと同一。適用不要。
        println!("適用済み pin と同一のため update は不要です（rev {current_pin}）");
        return Ok(());
    }

    // 適用「前」に config flake.lock の推移的 nixpkgs rev（old nixpkgs rev）を解決する。これが catch-up
    // 要約の選択起点になる。`last-applied-rev`（dotfiles pin）は適用要否 dedup 専用であり、要約選択には
    // 使わない（要約は `nixpkgs_old` と突合するため、dotfiles pin SHA を渡すと名前空間が違い恒久 miss する）。
    let previous_nixpkgs_rev = read_nixpkgs_rev(&config_dir)?;

    apply(&config_dir, &options, dry_run)?;

    // `--defer-rev-marker`: home/darwin を別ステップで適用する daemon ラッパー向けに、rev マーカー書込みを
    // ここでは行わず、darwin 成功後の `--commit-rev-marker` 起動へ委ねる。これにより darwin 失敗時に rev が
    // 適用済みと誤記録されて次回 skip し drift する（darwin 未収束のまま放置）問題を防ぐ。defer 時も適用後
    // 要約は表示する（home 適用は実際に進んでいるため）。
    if !options.defer_rev_marker {
        write_last_applied_rev(&state_dir, &current_pin, dry_run)?;
    }
    present_summary(
        &config_dir,
        &state_dir,
        Some(previous_nixpkgs_rev.as_str()),
        dry_run,
    )?;
    Ok(())
}

/// 既存の switch と同じ対象へ、先に flake.lock を更新してから適用する。
///
/// auto 経路は適用要否を判定済みで、この関数は実適用（lock 更新 + switch）だけを担う。`--dry-run` では
/// `process::run` がコマンド表示のみで実行するため、実際の lock 更新・switch・状態書込みは起きない。
fn apply(config_dir: &Path, options: &UpdateOptions, dry_run: bool) -> Result<()> {
    update_lock(config_dir, options.full, dry_run)?;
    switch::run(options.switch.clone())
}

/// ローカル flake の lock を更新する。
///
/// 既定では `nix flake update dotfiles --flake <DIR>` を実行し、dotfiles input だけを解決し直す。
/// これにより推移的 nixpkgs は dotfiles repo の committed lock に追従し、利用者ローカルでの暗黙の
/// nixpkgs 更新を避ける。`full` 指定時のみ input 名を省き、従来どおり全入力を最新解決で lock し直す。
fn update_lock(config_dir: &Path, full: bool, dry_run: bool) -> Result<()> {
    run_process("nix", update_args(config_dir, full), dry_run)
}

/// `nix flake update` の引数列を組み立てる純粋関数。
///
/// 既定では dotfiles input 名を含め、`full` 指定時は input 名を省いて全入力更新へフォールバックする。
/// 引数生成を実行から切り離すことで、`--full` の有無で引数列が変わることを単体検証できる。
fn update_args(config_dir: &Path, full: bool) -> Vec<OsString> {
    let mut args = vec![OsString::from("flake"), OsString::from("update")];
    if !full {
        args.push(OsString::from(local_flake::INPUT_NAME));
    }
    args.push(OsString::from("--flake"));
    args.push(config_dir.as_os_str().to_os_string());
    args
}

/// ユーザ所有の state dir（`$XDG_STATE_HOME/dotfiles`、未設定なら `$HOME/.local/state/dotfiles`）を返す。
///
/// XDG Base Directory に従い `XDG_STATE_HOME` を尊重し、未設定/空なら `$HOME/.local/state` を基底にする。
/// root 専用パスや共有可変領域は使わず、必ず呼び出しユーザの HOME 配下を指す（マーカーのユーザ所有保証）。
fn state_dir() -> Result<PathBuf> {
    let xdg = std::env::var_os("XDG_STATE_HOME");
    let home = std::env::var_os("HOME");
    resolve_state_dir(xdg, home)
}

/// XDG/HOME の env 値から state dir を決める純粋関数（解決規則を env 参照から切り離してテスト可能にする）。
///
/// `XDG_STATE_HOME` が非空ならそれを基底に、未設定/空なら `$HOME/.local/state` を基底にし、末尾へ `dotfiles`
/// を足す。`HOME` フォールバックが必要で `HOME` も無い場合は失敗にする（state dir 不定で誤って root 領域へ
/// 書かないため）。
fn resolve_state_dir(xdg_state_home: Option<OsString>, home: Option<OsString>) -> Result<PathBuf> {
    let base = match xdg_state_home {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| anyhow!("HOME is required"))?
            .join(".local")
            .join("state"),
    };
    Ok(base.join("dotfiles"))
}

/// 生成ローカル flake の `flake.lock` から dotfiles input の locked rev（= 現在の repo pin）を読む。
///
/// `nodes.<INPUT_NAME>.locked.rev` を repo pin として扱う。これは各マシンが追随する dotfiles repo の
/// 適用対象リビジョンであり、`last-applied-rev` との比較で適用要否を決める。lock 不在・構造不一致・rev 欠落は
/// 失敗にする（適用要否を誤判定して未適用/重複適用に倒さないため）。
fn read_repo_pin(config_dir: &Path) -> Result<String> {
    let lock_path = config_dir.join("flake.lock");
    let text = fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    parse_repo_pin(&text, local_flake::INPUT_NAME)
        .with_context(|| format!("failed to resolve repo pin from {}", lock_path.display()))
}

/// `flake.lock` JSON テキストから指定 input の locked rev を抽出する純粋関数。
///
/// 抽出経路を実行から切り離し、lock JSON 構造（`nodes.<input>.locked.rev`）の解釈を単体検証できるようにする。
fn parse_repo_pin(lock_text: &str, input_name: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(lock_text)?;
    value
        .get("nodes")
        .and_then(|nodes| nodes.get(input_name))
        .and_then(|node| node.get("locked"))
        .and_then(|locked| locked.get("rev"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("nodes.{input_name}.locked.rev not found"))
}

/// 生成ローカル flake の `flake.lock` から、dotfiles input が引き込む推移的 nixpkgs の locked rev を読む。
///
/// 更新履歴の `nixpkgs_old`/`nixpkgs_new` は nixpkgs rev であり、catch-up 要約の選択（`select_entries`）は
/// この nixpkgs rev と突合する。適用要否 dedup に使う dotfiles repo pin（`read_repo_pin`）とは名前空間が
/// 異なるため、要約選択の起点は本関数が返す nixpkgs rev を使う。lock 不在・構造不一致・rev 欠落は失敗にする
/// （誤った起点で要約が崩れないよう、解決不能を黙って空起点へ倒さない）。
fn read_nixpkgs_rev(config_dir: &Path) -> Result<String> {
    let lock_path = config_dir.join("flake.lock");
    let text = fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    parse_nixpkgs_rev(&text, local_flake::INPUT_NAME)
        .with_context(|| format!("failed to resolve nixpkgs rev from {}", lock_path.display()))
}

/// `flake.lock` JSON から、`input_name` node が引き込む推移的 nixpkgs node の locked rev を抽出する純粋関数。
///
/// 生成ローカル flake では root の input は dotfiles だけで、nixpkgs は dotfiles の推移依存として別 node 名
/// （例 `nixpkgs`）で現れる。`nodes.<input>.inputs.nixpkgs` は依存先 node 名（文字列、または follows を表す
/// 配列の先頭文字列）を指すため、それを辿って `nodes.<解決した node 名>.locked.rev` を返す。抽出を実行から
/// 切り離し、lock 構造の解釈（input 参照の解決）を単体検証できるようにする。
fn parse_nixpkgs_rev(lock_text: &str, input_name: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(lock_text)?;
    let nodes = value
        .get("nodes")
        .ok_or_else(|| anyhow!("flake.lock has no nodes object"))?;
    // dotfiles node の inputs.nixpkgs が指す node 名を解決する。値は node 名文字列か、follows を表す
    // 配列（先頭が node 名）のいずれか。どちらも先頭文字列を node 名として扱う。
    let nixpkgs_ref = nodes
        .get(input_name)
        .and_then(|node| node.get("inputs"))
        .and_then(|inputs| inputs.get("nixpkgs"))
        .ok_or_else(|| anyhow!("nodes.{input_name}.inputs.nixpkgs not found"))?;
    let nixpkgs_node_name = match nixpkgs_ref {
        serde_json::Value::String(name) => name.as_str(),
        serde_json::Value::Array(items) => items
            .first()
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("nodes.{input_name}.inputs.nixpkgs is an empty follows path"))?,
        _ => {
            return Err(anyhow!(
                "nodes.{input_name}.inputs.nixpkgs has unexpected shape"
            ));
        }
    };
    nodes
        .get(nixpkgs_node_name)
        .and_then(|node| node.get("locked"))
        .and_then(|locked| locked.get("rev"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("nodes.{nixpkgs_node_name}.locked.rev not found"))
}

/// `last-applied-rev` を読む（不存在/空なら `None`）。
fn read_last_applied_rev(state_dir: &Path) -> Result<Option<String>> {
    let path = state_dir.join(LAST_APPLIED_REV);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let rev = text.trim();
            Ok(if rev.is_empty() {
                None
            } else {
                Some(rev.to_string())
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(anyhow::Error::from(error).context(format!("failed to read {}", path.display())))
        }
    }
}

/// 適用済み rev を temp→rename で原子的に書き込む（ユーザ所有）。`--dry-run` では書かない。
///
/// 部分書込み（途中で観測される不完全 rev）を避けるため、同一 dir 内の temp ファイルへ書いてから rename する。
/// rename は同一ファイルシステム内で原子的であり、読み手は旧 rev か新 rev のどちらかだけを観測する。
fn write_last_applied_rev(state_dir: &Path, rev: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    let final_path = state_dir.join(LAST_APPLIED_REV);
    let temp_path = state_dir.join(format!("{LAST_APPLIED_REV}.{}.tmp", std::process::id()));
    fs::write(&temp_path, format!("{rev}\n"))
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, &final_path)
        .with_context(|| format!("failed to atomically replace {}", final_path.display()))?;
    Ok(())
}

/// 適用後の要約を catch-up 集約し、tty なら stdout、非 tty なら `pending-summary` へ振り分ける。
///
/// `nixpkgs_from_rev` を catch-up 区間の起点（その nixpkgs rev を適用前状態とする）に使い、複数 bump を
/// 跨いだ適用をアプリ単位で集約した重要度連動表示にする（描画と集約は `update_history` の show 経路を
/// 再利用）。起点は dotfiles repo pin ではなく **nixpkgs rev** である（要約選択は `nixpkgs_old` と突合する
/// ため）。stdout が tty なら起動元端末へ直接出力、非 tty（background daemon）なら `pending-summary` へ
/// **追記**して次回シェルで 1 回だけ消費させる（rev 単位の未表示分を失わないため上書きしない）。要約は
/// `last-run.log` へも残す。`--dry-run` では `pending-summary`/`last-run.log` へ書かず、tty 経路は stdout
/// 表示のみ行う。
fn present_summary(
    config_dir: &Path,
    state_dir: &Path,
    nixpkgs_from_rev: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let source = config_dir.join(HISTORY_SUBDIR);

    if std::io::stdout().is_terminal() {
        // tty: 起動元端末へ直接表示。stdout を sink にして show 描画を再利用する。
        update_history::render_applied_summary(&source, nixpkgs_from_rev, std::io::stdout())?;
        if !dry_run {
            append_last_run_log(state_dir, config_dir, nixpkgs_from_rev)?;
        }
        return Ok(());
    }

    // 非 tty（background）: pending-summary へ rev 単位で追記し、次回シェルが 1 回だけ消費する。
    if dry_run {
        // dry-run でもファイル契約を観測できるよう、捕捉バッファへ描画して破棄する（副作用なし）。
        let mut buffer = Vec::new();
        update_history::render_applied_summary(&source, nixpkgs_from_rev, &mut buffer)?;
        return Ok(());
    }
    append_pending_summary(state_dir, &source, nixpkgs_from_rev)?;
    append_last_run_log(state_dir, config_dir, nixpkgs_from_rev)?;
    Ok(())
}

/// `pending-summary` へ適用要約ブロックを追記する（上書きしない）。
///
/// 非 tty 適用ごとに 1 ブロックを末尾へ足す。daemon が連続適用しても未表示 rev を失わないよう追記で運用し、
/// 消費（表示と削除）は zsh フック（`config/zsh/auto-update.zsh`）が原子的 rename で 1 回だけ行うファイル
/// 契約とする。
fn append_pending_summary(
    state_dir: &Path,
    source: &Path,
    nixpkgs_from_rev: Option<&str>,
) -> Result<()> {
    let path = state_dir.join(PENDING_SUMMARY);
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    update_history::render_applied_summary(source, nixpkgs_from_rev, &file)?;
    Ok(())
}

/// `last-run.log` へ適用要約を残す（適用経路の出力記録）。
///
/// 適用の人間可読な要約を上書き保存し、直近 1 回分の適用内容を後から確認できるようにする。
fn append_last_run_log(
    state_dir: &Path,
    config_dir: &Path,
    nixpkgs_from_rev: Option<&str>,
) -> Result<()> {
    let path = state_dir.join(LAST_RUN_LOG);
    let source = config_dir.join(HISTORY_SUBDIR);
    let file =
        fs::File::create(&path).with_context(|| format!("failed to create {}", path.display()))?;
    update_history::render_applied_summary(&source, nixpkgs_from_rev, &file)?;
    Ok(())
}

/// `update.lock` の `O_EXCL` ベース排他ロック。drop でロックファイルを除去する。
///
/// flock(2) を使うと `libc` 直呼び（禁止）か新規 crate が要るため、移植性とテスト容易性を優先し
/// `create_new`（`O_CREAT|O_EXCL`）でロックファイルを作る方式を採る。作成成功＝ロック取得、`AlreadyExists`＝
/// 取得失敗（他プロセス適用中）として skip する。lock ファイルはユーザ所有 state dir 配下に作り、drop で除去する。
/// `--dry-run` では実ロックファイルを作らず（副作用なし）、常に取得成功として判定経路を通す。
struct UpdateLock {
    /// 取得したロックファイルのパス（drop で除去する）。`None` は dry-run（実ファイル無し）。
    path: Option<PathBuf>,
}

impl UpdateLock {
    /// ロックを非ブロッキングで試行する。取得成功で `Some`、既存ロックで `None` を返す。
    fn try_acquire(state_dir: &Path, dry_run: bool) -> Result<Option<Self>> {
        if dry_run {
            return Ok(Some(Self { path: None }));
        }
        let path = state_dir.join(LOCK_FILE);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                // 診断用に pid を書く（読み手はいないが、孤児ロックの調査に使える）。失敗は致命ではない。
                let _ = writeln!(file, "{}", std::process::id());
                Ok(Some(Self { path: Some(path) }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(anyhow::Error::from(error)
                .context(format!("failed to acquire lock {}", path.display()))),
        }
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        // ロックファイルを除去して次回取得を許す。除去失敗は次回 `create_new` を妨げるが、ここでは
        // panic させず黙って通す（drop 中の失敗伝播は不可）。孤児ロックは次回診断対象。
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}

#[derive(Args, Clone)]
/// ローカル flake の入力を更新してから、既存の switch と同じ対象を適用する。
pub(crate) struct UpdateOptions {
    #[command(flatten)]
    switch: switch::SwitchOptions,
    /// dotfiles input だけでなくローカル flake の全入力を最新解決で更新する。
    #[arg(long)]
    full: bool,
    /// 適用後に `last-applied-rev` を書かず、rev マーカー確定を後続の `--commit-rev-marker` へ委ねる。
    ///
    /// home/darwin を別ステップで適用する daemon ラッパー向け。home 適用ステップでこれを指定し、darwin 成功
    /// 後に `--commit-rev-marker` で rev を確定することで、darwin 失敗時の rev drift（未収束のまま skip）を防ぐ。
    #[arg(long, conflicts_with = "commit_rev_marker")]
    defer_rev_marker: bool,
    /// 適用・要約をせず、現在 pin を `last-applied-rev` へ確定書込みするだけの経路。
    ///
    /// `--defer-rev-marker` で適用した home に続き darwin が成功した後、ラッパーがこれを呼んで rev を確定する。
    #[arg(long, conflicts_with = "defer_rev_marker")]
    commit_rev_marker: bool,
}

#[cfg(test)]
mod tests {
    //! auto 経路の引数列・pin 解析・state dir 解決・last-applied-rev 原子書込み・lock 競合 skip を固定する。

    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use std::ffi::OsString as TestOsString;

    use super::{
        LOCK_FILE, PENDING_SUMMARY, UpdateLock, append_pending_summary, parse_nixpkgs_rev,
        parse_repo_pin, present_summary, read_last_applied_rev, resolve_state_dir, update_args,
        write_last_applied_rev,
    };

    /// 引数列を比較しやすいよう `OsString` を文字列へ揃える。
    fn as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    /// テスト専用の一時ディレクトリ（TMPDIR 配下）を作る。
    fn temp_dir(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("dotfiles-update-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp state dir");
        dir
    }

    #[test]
    fn default_updates_dotfiles_input_only() {
        // 既定では dotfiles input 名を渡し、推移的 nixpkgs を repo lock に追従させる。
        let args = update_args(Path::new("/cfg"), false);
        assert_eq!(
            as_strings(&args),
            vec!["flake", "update", "dotfiles", "--flake", "/cfg"]
        );
    }

    #[test]
    fn full_updates_all_inputs() {
        // `--full` では input 名を省き、従来の全入力更新へフォールバックする。
        let args = update_args(Path::new("/cfg"), true);
        assert_eq!(
            as_strings(&args),
            vec!["flake", "update", "--flake", "/cfg"]
        );
    }

    #[test]
    fn parse_repo_pin_reads_dotfiles_locked_rev() -> crate::Result<()> {
        // 生成ローカル flake の lock 構造（nodes.dotfiles.locked.rev）から repo pin を取り出す。
        let lock = r#"{
          "nodes": {
            "dotfiles": {
              "locked": { "rev": "abc123", "type": "github" }
            },
            "nixpkgs": {
              "locked": { "rev": "zzz999", "type": "github" }
            }
          },
          "root": "root",
          "version": 7
        }"#;
        assert_eq!(parse_repo_pin(lock, "dotfiles")?, "abc123");
        Ok(())
    }

    #[test]
    fn parse_repo_pin_fails_when_rev_missing() {
        let lock = r#"{ "nodes": { "dotfiles": { "locked": {} } } }"#;
        assert!(parse_repo_pin(lock, "dotfiles").is_err());
    }

    #[test]
    fn state_dir_respects_xdg_then_falls_back_to_home() -> crate::Result<()> {
        // XDG_STATE_HOME 非空ならそれを尊重する。
        assert_eq!(
            resolve_state_dir(Some(TestOsString::from("/xdg/state")), None)?,
            PathBuf::from("/xdg/state/dotfiles")
        );
        // 空 XDG は未設定扱いで HOME フォールバックする。
        assert_eq!(
            resolve_state_dir(
                Some(TestOsString::from("")),
                Some(TestOsString::from("/home/u"))
            )?,
            PathBuf::from("/home/u/.local/state/dotfiles")
        );
        // 未設定 XDG は HOME フォールバックする。
        assert_eq!(
            resolve_state_dir(None, Some(TestOsString::from("/home/u")))?,
            PathBuf::from("/home/u/.local/state/dotfiles")
        );
        // XDG も HOME も無ければ失敗（state dir 不定で root 領域へ書かない）。
        assert!(resolve_state_dir(None, None).is_err());
        Ok(())
    }

    #[test]
    fn last_applied_rev_round_trips_atomically() -> crate::Result<()> {
        let dir = temp_dir("rev");
        // 未書込みは None。
        assert_eq!(read_last_applied_rev(&dir)?, None);

        write_last_applied_rev(&dir, "rev-new", false)?;
        assert_eq!(read_last_applied_rev(&dir)?, Some("rev-new".to_string()));
        // temp ファイルは rename 後に残らない。
        let leftover: Vec<_> = std::fs::read_dir(&dir)
            .expect("read state dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftover.is_empty());

        // dry-run は書き込まない。
        write_last_applied_rev(&dir, "rev-dryrun", true)?;
        assert_eq!(read_last_applied_rev(&dir)?, Some("rev-new".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn lock_is_exclusive_until_dropped() -> crate::Result<()> {
        let dir = temp_dir("lock");
        let _ = std::fs::remove_file(dir.join(LOCK_FILE));

        let first = UpdateLock::try_acquire(&dir, false)?;
        assert!(first.is_some());
        // 競合: 取得済みのため None（skip）。
        assert!(UpdateLock::try_acquire(&dir, false)?.is_none());

        // 解放後は再取得できる。
        drop(first);
        let second = UpdateLock::try_acquire(&dir, false)?;
        assert!(second.is_some());
        drop(second);

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn dry_run_lock_has_no_file_side_effect() -> crate::Result<()> {
        let dir = temp_dir("lock-dry");
        let lock = UpdateLock::try_acquire(&dir, true)?;
        assert!(lock.is_some());
        // dry-run は実ロックファイルを作らない。
        assert!(!dir.join(LOCK_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 生成ローカル flake の lock テキスト（root input は dotfiles のみ、nixpkgs は dotfiles の推移依存）。
    /// dotfiles pin SHA と nixpkgs rev を**別値**にして、要約選択が pin ではなく nixpkgs rev を使うことを
    /// 検証できるようにする。
    fn local_flake_lock(dotfiles_pin: &str, nixpkgs_rev: &str, follows_array: bool) -> String {
        let nixpkgs_ref = if follows_array {
            r#"["nixpkgs"]"#.to_string()
        } else {
            r#""nixpkgs""#.to_string()
        };
        format!(
            r#"{{
              "nodes": {{
                "dotfiles": {{
                  "inputs": {{ "nixpkgs": {nixpkgs_ref} }},
                  "locked": {{ "rev": "{dotfiles_pin}", "type": "github" }}
                }},
                "nixpkgs": {{
                  "locked": {{ "rev": "{nixpkgs_rev}", "type": "github" }}
                }},
                "root": {{ "inputs": {{ "dotfiles": "dotfiles" }} }}
              }},
              "root": "root",
              "version": 7
            }}"#
        )
    }

    #[test]
    fn parse_nixpkgs_rev_follows_transitive_node_not_dotfiles_pin() -> crate::Result<()> {
        // 重大1 回帰: dotfiles pin（dedup 用）と nixpkgs rev（要約選択用）は別名前空間。
        // 要約選択へ pin を渡すと nixpkgs_old と恒久不一致になるため、推移的 nixpkgs rev を解決する。
        let lock = local_flake_lock("dotfilespin111", "nixpkgsrev999", false);
        // pin と nixpkgs rev を取り違えないこと。
        assert_eq!(parse_repo_pin(&lock, "dotfiles")?, "dotfilespin111");
        assert_eq!(parse_nixpkgs_rev(&lock, "dotfiles")?, "nixpkgsrev999");
        assert_ne!(
            parse_repo_pin(&lock, "dotfiles")?,
            parse_nixpkgs_rev(&lock, "dotfiles")?
        );
        Ok(())
    }

    #[test]
    fn parse_nixpkgs_rev_resolves_follows_array_reference() -> crate::Result<()> {
        // inputs.nixpkgs が follows を表す配列（先頭が node 名）でも node を解決できる。
        let lock = local_flake_lock("pinAAA", "revBBB", true);
        assert_eq!(parse_nixpkgs_rev(&lock, "dotfiles")?, "revBBB");
        Ok(())
    }

    #[test]
    fn parse_nixpkgs_rev_fails_when_input_wiring_missing() {
        // dotfiles node に inputs.nixpkgs が無ければ要約起点を誤らないよう失敗にする。
        let lock = r#"{ "nodes": { "dotfiles": { "locked": { "rev": "x" } } }, "root": "root" }"#;
        assert!(parse_nixpkgs_rev(lock, "dotfiles").is_err());
    }

    /// テスト用に `<dir>/docs/update-history/2026-06.toml` を書く。各エントリは `nixpkgs_old -> nixpkgs_new`
    /// のチェーンで、宣言アプリ 1 件を持つ（show 既定の宣言フィルタを通す）。
    fn write_history(config_dir: &Path, chain: &[(&str, &str)]) {
        let history_dir = config_dir.join(super::HISTORY_SUBDIR);
        std::fs::create_dir_all(&history_dir).expect("create history dir");
        let mut toml = String::new();
        for (old, new) in chain {
            toml.push_str(&format!(
                "[[update]]\n\
                 at = \"2026-06-05T00:00:00Z\"\n\
                 nixpkgs_old = \"{old}\"\n\
                 nixpkgs_new = \"{new}\"\n\
                 reference = \"darwinConfigurations.ci\"\n\
                 severity = \"minor\"\n\
                 overall = \"1アプリ更新\"\n\
                 \n\
                 [[update.package]]\n\
                 name = \"neovim-{old}\"\n\
                 old = \"1.0\"\n\
                 new = \"1.1\"\n\
                 change = \"upgraded\"\n\
                 declared = true\n\
                 \n\
                 [[update.package.change_item]]\n\
                 category = \"feature\"\n\
                 text = \"新機能 {old}\"\n\n"
            ));
        }
        std::fs::write(history_dir.join("2026-06.toml"), toml).expect("write history toml");
    }

    #[test]
    fn present_summary_selects_span_by_nixpkgs_rev_not_dotfiles_pin() -> crate::Result<()> {
        // 重大1 を踏むケース: dotfiles pin と nixpkgs rev が別値でも、nixpkgs old rev を起点に渡せば
        // catch-up span が正しく選ばれて要約が空にならない。pin SHA を起点にしていた旧実装では恒久空だった。
        let dir = temp_dir("present-nixpkgs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create state+config dir");
        // 適用前 nixpkgs rev = "nA"。チェーンは nA->nB, nB->nC（2 bump catch-up）。
        write_history(&dir, &[("nA", "nB"), ("nB", "nC")]);

        // 非 tty 経路（CI/テストは非 tty）で pending-summary へ追記される。nixpkgs old rev "nA" を渡す。
        present_summary(&dir, &dir, Some("nA"), false)?;
        let pending = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).expect("read pending");
        // 起点 "nA" 以降の 2 エントリ（2 アプリ）が集約表示される。空でないこと（バグ修正の核）。
        assert!(
            !pending.trim().is_empty(),
            "summary must not be empty: {pending}"
        );
        assert!(pending.contains("neovim-nA"), "{pending}");
        assert!(pending.contains("neovim-nB"), "{pending}");

        // 逆に dotfiles pin（nixpkgs_old に無い値）を起点にすると span は空＝旧バグ挙動を再現できる。
        let dir2 = temp_dir("present-pin");
        let _ = std::fs::remove_dir_all(&dir2);
        std::fs::create_dir_all(&dir2).expect("create dir2");
        write_history(&dir2, &[("nA", "nB"), ("nB", "nC")]);
        present_summary(&dir2, &dir2, Some("dotfilespin-not-a-nixpkgs-rev"), false)?;
        let empty = std::fs::read_to_string(dir2.join(PENDING_SUMMARY)).unwrap_or_default();
        // 起点が nixpkgs_old に一致しないと span 空（宣言アプリ行が出ない）。
        assert!(
            !empty.contains("neovim-"),
            "pin-as-rev should select empty span: {empty}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
        Ok(())
    }

    #[test]
    fn append_pending_summary_accumulates_rev_blocks() -> crate::Result<()> {
        // 非 tty 連続適用で pending-summary が rev 単位に**追記**累積し、未表示 rev を失わないことを実ファイルで固定。
        let dir = temp_dir("pending-accumulate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        write_history(&dir, &[("r1", "r2"), ("r2", "r3")]);
        let source = dir.join(super::HISTORY_SUBDIR);

        // 1 回目: r1 起点（r1->r2, r2->r3 の 2 ブロック相当を 1 show として追記）。
        append_pending_summary(&dir, &source, Some("r1"))?;
        let after_first = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).expect("read");
        assert!(after_first.contains("neovim-r1"));

        // 2 回目: r2 起点を追記。上書きされず先頭ブロックが残る（累積）。
        append_pending_summary(&dir, &source, Some("r2"))?;
        let after_second = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).expect("read");
        assert!(
            after_second.contains("neovim-r1"),
            "first block must remain: {after_second}"
        );
        assert!(
            after_second.contains("neovim-r2"),
            "second block appended: {after_second}"
        );
        assert!(
            after_second.len() > after_first.len(),
            "append must grow file, not overwrite"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn write_last_applied_rev_defer_then_commit_round_trips() -> crate::Result<()> {
        // 付随（drift 防止）: defer 経路では rev を書かず、commit で確定する分離をマーカー I/O として固定する。
        // run() の home/darwin 二段は実適用を要するため、ここでは「適用後 marker を遅延し後で確定する」契約の
        // 核（write/read の前後関係）を直接 assert する。
        let dir = temp_dir("defer-commit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        // defer: marker 未書込み（None のまま）。
        assert_eq!(read_last_applied_rev(&dir)?, None);
        // commit: 現在 pin を確定。
        write_last_applied_rev(&dir, "pin-after-darwin", false)?;
        assert_eq!(
            read_last_applied_rev(&dir)?,
            Some("pin-after-darwin".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}

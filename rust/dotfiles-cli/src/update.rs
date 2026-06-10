//! `dotfiles update` の auto 適用経路。ローカル flake の repo pin を追随し、必要時だけ適用する。
//!
//! `switch` は lock 済みの入力をそのまま使う。`update` は repo pin（生成ローカル flake の `flake.lock` に
//! おける dotfiles input の locked rev）が前回適用済み rev と異なるときだけ、`flake update` + switch を実行
//! して fleet を repo pin へ収束させる。
//!
//! ## 更新経路と状態
//!
//! 更新経路は 1 本（launchd timer が同じ `dotfiles update` を呼ぶ）で同時更新者を作らないため、排他・ロック・
//! scope 別 marker を持たない。万一の同時実行は nix 自身の store/profile ロックと冪等性に委ねる。状態は単一
//! marker `last-applied-rev`（適用済み dotfiles pin）と要約済みカーソル `last-summarized-at` だけである。
//!
//! ## 適用要否判定は lock 更新「後」に行う（fleet 追随の根幹）
//!
//! ローカル flake の `flake.lock` における dotfiles pin は、`nix flake update dotfiles` を実行するまで前回適用
//! 値のまま動かない。更新前に「ローカル pin == `last-applied-rev` なら skip」を判定すると、定常状態で常に skip
//! してマシンが新しい repo pin を永久に発見できない。これを避け、本 module は **先に `nix flake update` で
//! ローカル lock を最新 repo pin へ更新**し、更新後の pin を読んで `last-applied-rev` と比較する。
//!
//! ## 状態ディレクトリ
//!
//! 状態は `XDG_STATE_HOME` 非依存の固定 `$HOME/.local/state/dotfiles` に置く。auto-update の launchd daemon は
//! launchd の clean env で動き、利用者がインタラクティブシェルで設定する `XDG_STATE_HOME` を見られないため、daemon・
//! 手動 CLI・shell hook の state dir を一致させるには XDG を参照せず HOME 基準に固定するしかない。daemon は
//! darwin-rebuild を root で走らせるため **root のまま**（`HOME` をユーザ home に向けて）このバイナリを呼ぶ。

use std::ffi::OsString;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::Args;

use crate::{
    Result, local_flake, process::run as run_process, process::run_capture, switch, update_history,
};

/// 適用済み dotfiles repo pin を控える単一 marker（ユーザ所有）。pin が同じなら何もしない（冪等）。
const LAST_APPLIED_REV: &str = "last-applied-rev";
/// 最後に要約を表示/追記し終えた履歴エントリの `at`（RFC3339）。catch-up 要約 span の `at` カーソル起点。
///
/// 要約「後」に書く。span 起点を nixpkgs rev でなく `at` にするのは、brew tap だけが進み
/// `nixpkgs_old == nixpkgs_new` の brew-only 更新が複数できても `at` は記録のたび前進する一意値であり、
/// 同じ更新を毎回再表示しないため（show-once）。
const LAST_SUMMARIZED_AT: &str = "last-summarized-at";
/// 非 tty 適用時に要約を追記し、次回シェルが 1 回だけ消費する累積ファイル。
const PENDING_SUMMARY: &str = "pending-summary";
/// 直近 1 回分の適用要約を残すログ。
const LAST_RUN_LOG: &str = "last-run.log";

/// 適用済み dotfiles flake input source 内の更新履歴ディレクトリ（`<source>/docs/update-history`）。
const HISTORY_SUBDIR: &str = "docs/update-history";
/// state dir 配下に複製した更新履歴のローカル複製先（`<state-dir>/history`）。show/要約はここを読む。
const HISTORY_LOCAL_SUBDIR: &str = "history";

/// `dotfiles update` の実行結果。cli が exit code へ変換する。
///
/// 排他を持たないため「適用した / up-to-date を確認した」を区別せず、いずれも `Completed`（成功）にする。
pub(crate) enum UpdateOutcome {
    /// 適用 / up-to-date 確認を完了した。exit 0。
    Completed,
}

/// auto 経路の入口。**先に lock を更新してから** repo pin を読み、前回適用済み rev と異なるときだけ適用する。
///
/// 順序: flake ファイルの symlink 拒否 → state dir 確保 → `nix flake update` で **ローカル lock を最新 repo pin へ
/// 更新**（`--dry-run` では非実行）→ 更新後の dotfiles pin を読む → `last-applied-rev` と同じなら switch / record /
/// marker を skip（冪等。ただし dry-run では lock 未更新で pin が古いため skip 判定はせず誤った『switch 不要』を出さない）→
/// 異なれば switch → 履歴をローカル複製 → 適用範囲の catch-up 要約を tty/非 tty で振り分け表示 → `last-applied-rev`
/// を原子的更新。skip 判定を lock 更新「後」に置くのは、ローカル pin が `nix flake update` 前は前回適用値のまま
/// 動かず、更新前判定だと定常状態で常に skip して fleet が nightly bump へ追随しなくなるためである。
pub(crate) fn run(options: UpdateOptions) -> Result<UpdateOutcome> {
    let config_dir = options.switch.config_dir()?;
    switch::ensure_config_exists(&config_dir)?;

    // lock を書き換える前に flake ファイルが symlink でないことを検査する。root daemon は root のまま
    // ユーザ所有 `~/.config/dotfiles` を `--config-dir` に渡すため、利用者が `flake.lock` を root 所有ファイルへの
    // symlink に差し替えると `nix flake update` が root 権限でリンク先を上書きさせられる（権限昇格）。dotfiles の
    // flake ファイルが symlink であることは正当でないため、root 実行に限らず常に拒否する。
    assert_flake_files_not_symlink(&config_dir)?;

    let state_dir = state_dir()?;
    let dry_run = options.switch.dry_run();

    if !dry_run {
        // 状態ファイルはユーザ所有の state dir 配下にしか作らない。
        fs::create_dir_all(&state_dir)
            .with_context(|| format!("failed to create state dir {}", state_dir.display()))?;
    }

    // **先に** ローカル lock を最新 repo pin へ更新する（skip 判定はこの後）。lock 更新前のローカル pin は前回
    // 適用値のまま動かないため、更新前に判定すると定常状態で常に skip し fleet が追随しない。
    update_lock(&config_dir, options.full, dry_run)?;

    // lock 更新後の dotfiles pin を読む。これが今回の適用対象（upstream の最新 repo pin）。
    let current_pin = read_repo_pin(&config_dir)?;
    let previous_rev = read_last_applied_rev(&state_dir)?;
    if dry_run {
        // dry-run では `update_lock` が `flake.lock` を更新しないため、`current_pin` は更新前の古い pin である。
        // pin 比較による「switch 不要」の早期 return は古い pin に基づく誤判定（定常状態で常に skip）になりうるため
        // 行わず、実行時には lock が最新 repo pin へ更新されてから判定される旨を明示する。
        println!(
            "dry-run: lock を更新していないため pin 比較による switch 要否判定は行いません\
             （現 pin {current_pin}。実行時は lock 更新後の pin で判定し、必要なら switch されます）"
        );
    } else if previous_rev.as_deref() == Some(current_pin.as_str()) {
        // 更新後の pin が前回適用済みと同一。switch / record / marker を skip する（lock 更新は実施済み）。
        println!("適用済み pin と同一のため switch は不要です（rev {current_pin}）");
        return Ok(UpdateOutcome::Completed);
    }

    // 更新後 pin が前回と異なる → switch を実行する（home+darwin を一度に。部分適用経路は持たない）。
    // `update` は target を受け取らず常に `SwitchTarget::All` で適用するため、部分適用後に全体 marker を
    // 確定する不整合（`dotfiles update home` で home だけ適用して全体 pin を確定する）が起き得ない。
    switch::apply(&options.switch, switch::SwitchTarget::All)?;

    // 適用済み dotfiles flake input source の `docs/update-history` を state dir のローカル複製へ取り込む。
    // 複製失敗（network 無し・解決不能等）は適用を止めず、要約と要約済み marker の確定だけを次回へ繰り越す。
    let history_synced = match sync_history(&config_dir, &state_dir, dry_run) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "更新履歴の複製に失敗しました（要約表示と要約済み rev の確定は次回へ繰り越します）: {error}"
            );
            false
        }
    };

    // 要約を表示/追記し、その後で marker（要約済み `at`）を確定する。順序が要点で、要約「前」に marker を進めると
    // partial-failure（switch 後・要約前に異常終了）で未表示範囲を失う。履歴複製が失敗したときは要約を skip し、
    // span 起点を保って次回再試行に委ねる。
    if history_synced {
        present_and_commit_summary(&state_dir, dry_run)?;
    }

    // 適用済み marker を確定する（履歴複製・要約が成功した後にだけ）。要約失敗は上の `?` で伝播し、ここに到達
    // しないため marker を書かない。履歴複製失敗時（`history_synced == false`）も確定せず、次回同一 pin で
    // switch（冪等）→ 再同期 → 再要約を試せるようにする。
    if history_synced {
        write_rev_atomic(&state_dir.join(LAST_APPLIED_REV), &current_pin, dry_run)?;
    }
    Ok(UpdateOutcome::Completed)
}

/// `<config_dir>/flake.lock` と `<config_dir>/flake.nix` が symlink でないことを検査する（symlink なら `Err`）。
///
/// root daemon が root のままユーザ所有 config dir を扱う経路で、利用者が flake ファイルを root 所有ファイルへの
/// symlink へ差し替えると、`nix flake update` の `flake.lock` 書き換えが root 権限でリンク先を上書きさせられる
/// （権限昇格）。検査を `assert_not_symlink` の純粋判定に委ね、存在する flake ファイルがすべて通常ファイルである
/// ことを確かめる。
fn assert_flake_files_not_symlink(config_dir: &Path) -> Result<()> {
    ["flake.lock", "flake.nix"]
        .into_iter()
        .try_for_each(|name| assert_not_symlink(&config_dir.join(name)))
}

/// `path` が存在する場合に symlink でない（通常ファイル/dir である）ことを検査する純粋関数（不在は許容）。
///
/// `std::fs::symlink_metadata` は symlink 自体の metadata を返す（リンク先を辿らない）ため、symlink を確実に
/// 検出できる。`NotFound` は flake ファイルが無い正当な状態として許容し、それ以外の stat 失敗は伝播する。
/// `libc` を直呼びせず std のみで判定する。
fn assert_not_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(anyhow!(
            "{} が symlink です。dotfiles の flake ファイルが symlink であることは正当でないため停止します\
             （root 権限でのリンク先上書きによる権限昇格を防ぐ）",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::Error::from(error).context(format!(
            "failed to stat {} for symlink check",
            path.display()
        ))),
    }
}

/// ローカル flake の lock を更新する。
///
/// 既定では `nix flake update dotfiles --flake <DIR>` を実行し dotfiles input だけを解決し直す（推移的 nixpkgs は
/// dotfiles repo の committed lock に追従）。`full` 指定時のみ input 名を省き、全入力を最新解決で lock し直す。
fn update_lock(config_dir: &Path, full: bool, dry_run: bool) -> Result<()> {
    run_process("nix", update_args(config_dir, full), dry_run)
}

/// `nix flake update` の引数列を組み立てる純粋関数。
///
/// 既定では dotfiles input 名を含め、`full` 指定時は input 名を省いて全入力更新へフォールバックする。
fn update_args(config_dir: &Path, full: bool) -> Vec<OsString> {
    // `full` 指定時は input 名を省いて全入力更新へフォールバックする（既定は dotfiles input だけ）。
    let input_name = (!full).then(|| OsString::from(local_flake::INPUT_NAME));
    [OsString::from("flake"), OsString::from("update")]
        .into_iter()
        .chain(input_name)
        .chain([
            OsString::from("--flake"),
            config_dir.as_os_str().to_os_string(),
        ])
        .collect()
}

/// 適用済み dotfiles flake input source の `docs/update-history` を state dir のローカル複製へ取り込む。
///
/// `~/.config/dotfiles` のローカル flake は `flake.nix`/`flake.lock` だけを持ち更新履歴を含まないため、履歴は
/// **適用済み dotfiles input が指す store path** から複製する。複製先は `<state-dir>/history` で、以降の show/要約は
/// このローカル複製を offline・決定論で読む。`--dry-run` では複製しない。
fn sync_history(config_dir: &Path, state_dir: &Path, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    sync_history_from_source(resolve_input_source(config_dir), state_dir)
}

/// 解決済み source（`Some` = archive 成功、`None` = archive 失敗）から履歴複製の成否を決める分離点。
///
/// archive 失敗（`None`）は同期未成功として `Err` を返す（呼び出し側が要約を skip し marker を進めないため）。
/// `Some(source)` でも履歴 dir が無い場合は「複製対象が無い正常系」として `Ok(())`。
fn sync_history_from_source(source_root: Option<PathBuf>, state_dir: &Path) -> Result<()> {
    let source_root = source_root.ok_or_else(|| {
        anyhow!(
            "failed to resolve dotfiles input source via `nix flake archive` (history not synced; \
             summary deferred to next run)"
        )
    })?;
    let source_history = source_root.join(HISTORY_SUBDIR);
    if !source_history.is_dir() {
        return Ok(());
    }
    let dest = state_dir.join(HISTORY_LOCAL_SUBDIR);
    copy_history_dir(&source_history, &dest)
}

/// 適用済み dotfiles flake input の **realize 済み source store path** を解決する（解決不能なら `None`）。
///
/// `nix flake archive <config-dir> --json --no-write-lock-file` の `inputs.<dotfiles>.path` を返す。metadata の
/// `locked` でなく archive を使うのは、本番の github 型 input が metadata に `path` を持たないためである。
/// network 無し・nix 不在・archive 失敗・JSON 解析失敗はいずれも `None` へ縮退する（履歴複製は best-effort）。
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

/// `<source>/docs/update-history` の `*.toml` を `<state-dir>/history` へ複製する（atomic 置換）。
///
/// 複製前に dest を一時 dir へ新規構築 → 完成後に既存 dest と atomic に rename 置換する。読み手（show / 要約）は
/// 「古い完全な複製」か「新しい完全な複製」のどちらかだけを観測し、削除途中・コピー途中の中間状態を見ない。
/// 既存複製の喪失を避けるため、既存 dest を backup へ rename 退避 → temp を dest へ rename → 成功時に backup を
/// 削除、失敗時は backup を dest へ rename 復元する。`libc` を直呼びせず std の rename/remove のみで実現する。
fn copy_history_dir(source_history: &Path, dest: &Path) -> Result<()> {
    let parent = dest.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create history parent {}", parent.display()))?;
    let dest_name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| HISTORY_LOCAL_SUBDIR.to_string());
    let temp_dir = parent.join(format!("{dest_name}.sync.{}.tmp", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let build = (|| -> Result<()> {
        fs::create_dir_all(&temp_dir)
            .with_context(|| format!("failed to create history temp {}", temp_dir.display()))?;
        for entry in fs::read_dir(source_history)
            .with_context(|| format!("failed to read {}", source_history.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let from = entry.path();
            let to = temp_dir.join(entry.file_name());
            fs::copy(&from, &to).with_context(|| {
                format!("failed to copy {} to {}", from.display(), to.display())
            })?;
        }
        Ok(())
    })();
    if let Err(error) = build {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(error);
    }

    let backup_dir = parent.join(format!("{dest_name}.backup.{}.tmp", std::process::id()));
    replace_history_dir_atomically(&temp_dir, dest, &backup_dir)
}

/// 完成済み複製 `temp_dir` を、既存複製を喪失せずに `dest` へ差し替える。
///
/// 1. 既存 dest を `backup_dir` へ rename 退避（dest が無い初回は退避不要）。2. `temp_dir` を dest へ rename。
/// 3. 成功時のみ `backup_dir` を削除。4. 失敗時は temp を掃除し、退避した既存複製を dest へ rename 復元する。
fn replace_history_dir_atomically(temp_dir: &Path, dest: &Path, backup_dir: &Path) -> Result<()> {
    let _ = fs::remove_dir_all(backup_dir);
    let backed_up = fs::rename(dest, backup_dir).is_ok();
    if let Err(error) = fs::rename(temp_dir, dest) {
        let _ = fs::remove_dir_all(temp_dir);
        if backed_up {
            let _ = fs::rename(backup_dir, dest);
        }
        return Err(anyhow::Error::from(error).context(format!(
            "failed to atomically replace history dir {}",
            dest.display()
        )));
    }
    if backed_up {
        let _ = fs::remove_dir_all(backup_dir);
    }
    Ok(())
}

/// 更新履歴のローカル複製ディレクトリ（`<state-dir>/history`）を返す。
///
/// `update-history show`（`--source` 未指定時）が読む既定 source であり、`update` 経路と同一の state dir 解決
/// 規則（HOME 固定）を共有する。
pub(crate) fn history_local_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join(HISTORY_LOCAL_SUBDIR))
}

/// state dir（`$HOME/.local/state/dotfiles`）を返す。
fn state_dir() -> Result<PathBuf> {
    resolve_state_dir(std::env::var_os("HOME"))
}

/// HOME の env 値から state dir を決める純粋関数（解決規則を env 参照から切り離してテスト可能にする）。
///
/// `XDG_STATE_HOME` は参照せず `<HOME>/.local/state/dotfiles` に固定する。auto-update の launchd daemon は clean
/// env で動き利用者の interactive `XDG_STATE_HOME` を見られないため、XDG 依存だと daemon と shell hook の state dir
/// がずれて show-once（pending-summary 消費）が機能しない。HOME 基準に固定して daemon・手動 CLI・shell hook を一致
/// させる。HOME が無ければ `Err`。
fn resolve_state_dir(home: Option<OsString>) -> Result<PathBuf> {
    let base = home
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("HOME is required"))?
        .join(".local")
        .join("state");
    Ok(base.join("dotfiles"))
}

/// 生成ローカル flake の `flake.lock` から dotfiles input の現在の repo pin（適用要否 dedup の同一性）を読む。
fn read_repo_pin(config_dir: &Path) -> Result<String> {
    let lock_path = config_dir.join("flake.lock");
    let text = fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    parse_repo_pin(&text, local_flake::INPUT_NAME)
        .with_context(|| format!("failed to resolve repo pin from {}", lock_path.display()))
}

/// `flake.lock` JSON テキストから指定 input の repo pin 同一性を抽出する純粋関数。
///
/// pin 同一性は `locked.rev`（github source）→ `locked.narHash`（path source で rev が無い場合）→
/// `locked.lastModified`（数値も許容）の順で解決する。いずれも無ければ `Err`。
fn parse_repo_pin(lock_text: &str, input_name: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(lock_text)?;
    let locked = value
        .get("nodes")
        .and_then(|nodes| nodes.get(input_name))
        .and_then(|node| node.get("locked"))
        .ok_or_else(|| anyhow!("nodes.{input_name}.locked not found"))?;
    locked_pin_identity(locked).ok_or_else(|| {
        anyhow!("nodes.{input_name}.locked has no rev/narHash/lastModified pin identity")
    })
}

/// `locked` オブジェクトから repo pin 同一性を 1 つ解決する純粋関数（rev → narHash → lastModified の順）。
fn locked_pin_identity(locked: &serde_json::Value) -> Option<String> {
    if let Some(rev) = locked.get("rev").and_then(serde_json::Value::as_str) {
        return Some(rev.to_string());
    }
    if let Some(nar_hash) = locked.get("narHash").and_then(serde_json::Value::as_str) {
        return Some(nar_hash.to_string());
    }
    match locked.get("lastModified") {
        Some(serde_json::Value::String(value)) => Some(value.clone()),
        Some(serde_json::Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

/// `last-applied-rev` を読む（不存在/空なら `None`）。
fn read_last_applied_rev(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_APPLIED_REV))
}

/// 最後に要約を表示/追記し終えた履歴エントリの `at` を読む（不存在/空なら `None`）。
fn read_last_summarized_at(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_SUMMARIZED_AT))
}

/// rev/at 値ファイルを読み、trim して非空なら `Some` を返す（不存在/空は `None`）。
fn read_trimmed_rev(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let trimmed = text.trim();
            Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(anyhow::Error::from(error).context(format!("failed to read {}", path.display())))
        }
    }
}

/// rev/at を同一 dir 内 temp→rename で原子的に書く（部分書込みを観測させない）。`--dry-run` では書かない。
fn write_rev_atomic(final_path: &Path, value: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    let file_name = final_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| LAST_APPLIED_REV.to_string());
    let temp_path = final_path.with_file_name(format!("{file_name}.{}.tmp", std::process::id()));
    fs::write(&temp_path, format!("{value}\n"))
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, final_path)
        .with_context(|| format!("failed to atomically replace {}", final_path.display()))?;
    Ok(())
}

/// 適用後要約を表示/追記し、**成功時に** 要約済み marker（`last-summarized-at`）を終端 `at` へ進める。
///
/// span 起点（`after_at`）は `last-summarized-at` から読む。要約「後」に marker を進めることで、partial-failure
/// （要約中の異常終了）で未表示範囲を失わず、同日に複数回適用しても 2 回目は起点 = 終端 `at` → 空 span →
/// 再追記しない。tty 判定はここで 1 回だけ解決して `present_summary` へ注入する。
fn present_and_commit_summary(state_dir: &Path, dry_run: bool) -> Result<()> {
    let span_start_at = read_last_summarized_at(state_dir)?;
    let stdout_is_terminal = std::io::stdout().is_terminal();
    let summarized_at = present_summary(
        state_dir,
        span_start_at.as_deref(),
        dry_run,
        stdout_is_terminal,
    )?;
    if let Some(at) = summarized_at {
        write_rev_atomic(&state_dir.join(LAST_SUMMARIZED_AT), &at, dry_run)?;
    }
    Ok(())
}

/// 適用後の要約を catch-up 集約し、tty なら stdout、非 tty なら `pending-summary` へ振り分ける。
///
/// `summarized_after_at` を catch-up 区間の起点（その `at` より後のエントリを対象）に使う。`stdout_is_terminal`
/// が真なら起動元端末へ直接出力、偽（background）なら `pending-summary` へ **追記** して次回シェルで 1 回だけ消費
/// させる。要約は `last-run.log` へも残す。`--dry-run` ではファイルへ書かず描画経路だけを通す。戻り値は要約し
/// 終えた終端 `at`（次回カーソル。空 span なら `None`）。全体適用なので出所フィルタは常に `All`。
fn present_summary(
    state_dir: &Path,
    summarized_after_at: Option<&str>,
    dry_run: bool,
    stdout_is_terminal: bool,
) -> Result<Option<String>> {
    let source = state_dir.join(HISTORY_LOCAL_SUBDIR);

    if stdout_is_terminal {
        let summarized_at = update_history::render_applied_summary(
            &source,
            summarized_after_at,
            std::io::stdout(),
        )?;
        if !dry_run {
            append_last_run_log(state_dir, summarized_after_at)?;
        }
        return Ok(summarized_at);
    }

    if dry_run {
        // dry-run でも描画経路を通すが副作用は持たない（捕捉バッファへ描画して破棄する）。
        let (_bytes, summarized_at) = render_summary_bytes(&source, summarized_after_at)?;
        return Ok(summarized_at);
    }
    let summarized_at = append_pending_summary(state_dir, &source, summarized_after_at)?;
    append_last_run_log(state_dir, summarized_after_at)?;
    Ok(summarized_at)
}

/// 適用後要約を Vec バッファへ描画し、`(描画バイト列, 要約済み終端 at)` を返す。
///
/// `render_applied_summary` は `Write` sink へ書くため、ここで Vec の writer を 1 箇所に閉じ込める（呼び出し側へ
/// 可変バッファを露出しない）。dry-run の破棄描画と `pending-summary` への追記公開がこの 1 関数を共有する。
fn render_summary_bytes(
    source: &Path,
    summarized_after_at: Option<&str>,
) -> Result<(Vec<u8>, Option<String>)> {
    let mut buffer = Vec::new();
    let summarized_at =
        update_history::render_applied_summary(source, summarized_after_at, &mut buffer)?;
    Ok((buffer, summarized_at))
}

/// 既存 `pending-summary`（`path`）を `claim_path` へ atomic rename して所有権を取り、その内容を返す。
///
/// `path` が存在しなければ空（初回 publish）を返す。rename 後の read に失敗したときは claim を `path` へ戻して
/// 内容を失わせず `Err` を返す（戻し自体に失敗しても claim ファイルは残り、consumer から完全消失することは防ぐ）。
/// rename の所有権獲得が NotFound 以外で失敗したときも `Err`。
fn claim_existing_pending(path: &Path, claim_path: &Path) -> Result<Vec<u8>> {
    match fs::rename(path, claim_path) {
        Ok(()) => match fs::read(claim_path) {
            Ok(bytes) => Ok(bytes),
            Err(error) => {
                let _ = fs::rename(claim_path, path);
                Err(anyhow::Error::from(error)
                    .context(format!("failed to read claimed {}", claim_path.display())))
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => {
            Err(anyhow::Error::from(error).context(format!("failed to claim {}", path.display())))
        }
    }
}

/// `pending-summary` へ適用要約ブロックを追記公開する（上書きしない・完成済みブロックだけを公開する）。
///
/// 非 tty 適用ごとに 1 ブロックを足す（累積運用で未表示分を失わない）。消費（表示と削除）は zsh フックが原子的
/// rename で 1 回だけ行うファイル契約とする。consumer との rename 競合を避けるため、producer も consumer と同じ
/// rename による所有権獲得で publish する: (1) 既存 `pending-summary` を claim ファイルへ atomic rename して
/// 所有権を取る、(2) 取得した既存内容に新ブロックを連結して temp に完成させる、(3) temp を `pending-summary` へ
/// atomic rename で publish する。render 失敗時は live ファイルへ一切触れない。
fn append_pending_summary(
    state_dir: &Path,
    source: &Path,
    summarized_after_at: Option<&str>,
) -> Result<Option<String>> {
    let path = state_dir.join(PENDING_SUMMARY);
    let (rendered, summarized_at) = render_summary_bytes(source, summarized_after_at)?;

    let claim_path = path.with_file_name(format!(
        "{PENDING_SUMMARY}.appending.{}",
        std::process::id()
    ));
    let _ = fs::remove_file(&claim_path);
    let existing = claim_existing_pending(&path, &claim_path)?;

    let temp_path = path.with_file_name(format!(
        "{PENDING_SUMMARY}.publish.{}.tmp",
        std::process::id()
    ));
    let publish = (|| -> Result<()> {
        // 既存ブロックの後ろに今回ブロックを不変連結して 1 度に書き出す。
        let combined: Vec<u8> = existing
            .iter()
            .copied()
            .chain(rendered.iter().copied())
            .collect();
        fs::write(&temp_path, &combined)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        fs::rename(&temp_path, &path)
            .with_context(|| format!("failed to publish {}", path.display()))?;
        Ok(())
    })();

    match publish {
        Ok(()) => {
            let _ = fs::remove_file(&claim_path);
            Ok(summarized_at)
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            if existing.is_empty() {
                let _ = fs::remove_file(&claim_path);
            } else {
                let _ = fs::rename(&claim_path, &path);
            }
            Err(error)
        }
    }
}

/// `last-run.log` へ適用要約を残す（直近 1 回分の適用内容を後から確認できるようにする）。
fn append_last_run_log(state_dir: &Path, summarized_after_at: Option<&str>) -> Result<()> {
    let path = state_dir.join(LAST_RUN_LOG);
    let source = state_dir.join(HISTORY_LOCAL_SUBDIR);
    let file =
        fs::File::create(&path).with_context(|| format!("failed to create {}", path.display()))?;
    let _ = update_history::render_applied_summary(&source, summarized_after_at, &file)?;
    Ok(())
}

/// `dotfiles update` の利用者向け option。
///
/// **部分 target を受理しない**: `switch` の共通オプション（[`switch::SwitchCommon`]）だけを flatten し、適用対象
/// （`home`/`darwin`）を持たない。`update` は常に全体適用（home+darwin）であり、これにより部分適用後に全体
/// `last-applied-rev` を確定してしまう不整合（例: `dotfiles update home` で home だけ適用→daemon が同一 pin を
/// skip→darwin/system 未適用が残る）を構造的に排除する。
#[derive(Args)]
pub(crate) struct UpdateOptions {
    #[command(flatten)]
    switch: switch::SwitchCommon,
    /// dotfiles input だけでなくローカル flake の全入力を最新解決で更新する。
    #[arg(long)]
    full: bool,
}

#[cfg(test)]
mod tests {
    //! update の引数列・pin 解析・state dir 解決・marker 原子書込み・冪等判定を固定する。

    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{
        LAST_APPLIED_REV, UpdateOptions, assert_not_symlink, claim_existing_pending,
        parse_input_source_path, parse_repo_pin, read_last_applied_rev,
        replace_history_dir_atomically, resolve_state_dir, update_args, write_rev_atomic,
    };
    use anyhow::anyhow;
    use clap::Parser;

    #[derive(Parser)]
    struct TestUpdateCli {
        #[command(flatten)]
        update: UpdateOptions,
    }

    fn parse_update(args: &[&str]) -> crate::Result<UpdateOptions> {
        let argv: Vec<&str> = std::iter::once("dotfiles")
            .chain(args.iter().copied())
            .collect();
        TestUpdateCli::try_parse_from(argv)
            .map(|cli| cli.update)
            .map_err(|error| anyhow!("parse update options: {error}"))
    }

    fn as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn temp_dir(tag: &str) -> crate::Result<PathBuf> {
        let dir =
            std::env::temp_dir().join(format!("dotfiles-update-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|error| anyhow!("create dir: {error}"))?;
        Ok(dir)
    }

    fn twrite(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> crate::Result<()> {
        let path = path.as_ref();
        std::fs::write(path, contents).map_err(|error| anyhow!("write {}: {error}", path.display()))
    }

    fn tread(path: impl AsRef<Path>) -> crate::Result<String> {
        let path = path.as_ref();
        std::fs::read_to_string(path).map_err(|error| anyhow!("read {}: {error}", path.display()))
    }

    #[test]
    fn default_updates_dotfiles_input_only() -> crate::Result<()> {
        let options = parse_update(&[])?;
        let args = update_args(Path::new("/cfg"), options.full);
        assert_eq!(
            as_strings(&args),
            vec!["flake", "update", "dotfiles", "--flake", "/cfg"]
        );
        Ok(())
    }

    #[test]
    fn rejects_partial_target_so_update_is_always_full() {
        // 部分 target（`home` / `darwin`）は受理しない。`update` は常に全体適用であり、部分適用後に全体
        // `last-applied-rev` を確定する不整合を構造的に排除する。位置引数 `home`/`darwin` は parse error。
        assert!(parse_update(&["home"]).is_err());
        assert!(parse_update(&["darwin"]).is_err());
        assert!(parse_update(&["all"]).is_err());
    }

    #[test]
    fn full_updates_all_inputs() -> crate::Result<()> {
        let options = parse_update(&["--full"])?;
        let args = update_args(Path::new("/cfg"), options.full);
        // input 名を省いて全入力更新へフォールバックする。
        assert_eq!(
            as_strings(&args),
            vec!["flake", "update", "--flake", "/cfg"]
        );
        Ok(())
    }

    #[test]
    fn parse_repo_pin_prefers_rev_then_narhash_then_last_modified() -> crate::Result<()> {
        let with_rev = r#"{"nodes":{"dotfiles":{"locked":{"rev":"abc123","narHash":"sha","lastModified":1}}}}"#;
        assert_eq!(parse_repo_pin(with_rev, "dotfiles")?, "abc123");
        let with_nar =
            r#"{"nodes":{"dotfiles":{"locked":{"narHash":"sha256-x","lastModified":1}}}}"#;
        assert_eq!(parse_repo_pin(with_nar, "dotfiles")?, "sha256-x");
        let with_mtime = r#"{"nodes":{"dotfiles":{"locked":{"lastModified":1717000000}}}}"#;
        assert_eq!(parse_repo_pin(with_mtime, "dotfiles")?, "1717000000");
        // pin 同一性を表す値が一切無ければ Err。
        let none = r#"{"nodes":{"dotfiles":{"locked":{}}}}"#;
        assert!(parse_repo_pin(none, "dotfiles").is_err());
        Ok(())
    }

    #[test]
    fn parse_input_source_path_extracts_realized_store_path() {
        let json = r#"{"inputs":{"dotfiles":{"path":"/nix/store/abc-source"}}}"#;
        assert_eq!(
            parse_input_source_path(json, "dotfiles").as_deref(),
            Some("/nix/store/abc-source")
        );
        // path が無い形は None。
        let no_path = r#"{"inputs":{"dotfiles":{}}}"#;
        assert_eq!(parse_input_source_path(no_path, "dotfiles"), None);
    }

    #[test]
    fn resolve_state_dir_is_home_fixed_and_ignores_xdg() -> crate::Result<()> {
        // XDG_STATE_HOME は参照せず HOME 基準に固定する（daemon の clean env と shell hook の state dir 一致のため）。
        let resolved = resolve_state_dir(Some(OsString::from("/home/u")))?;
        assert_eq!(
            resolved,
            PathBuf::from("/home/u")
                .join(".local")
                .join("state")
                .join("dotfiles")
        );
        // HOME が無ければ Err。
        assert!(resolve_state_dir(None).is_err());
        // HOME が空でも Err。
        assert!(resolve_state_dir(Some(OsString::from(""))).is_err());
        Ok(())
    }

    #[test]
    fn write_rev_atomic_round_trips_and_dry_run_skips() -> crate::Result<()> {
        let dir = temp_dir("rev-atomic")?;
        let path = dir.join(LAST_APPLIED_REV);
        let _ = std::fs::remove_file(&path);
        write_rev_atomic(&path, "abc123", false)?;
        assert_eq!(read_last_applied_rev(&dir)?.as_deref(), Some("abc123"));
        // dry-run では書かない（既存値を変えない）。
        write_rev_atomic(&path, "def456", true)?;
        assert_eq!(read_last_applied_rev(&dir)?.as_deref(), Some("abc123"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn replace_history_dir_atomically_restores_old_copy_on_failure() -> crate::Result<()> {
        // 置換成功時は dest が新複製になる。dest 不在初回でも成功する。
        let dir = temp_dir("history-replace")?;
        let temp = dir.join("history.sync.tmp");
        let dest = dir.join("history");
        let backup = dir.join("history.backup.tmp");
        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&temp).map_err(|error| anyhow!("mkdir temp: {error}"))?;
        twrite(temp.join("2026-06.toml"), "new")?;
        replace_history_dir_atomically(&temp, &dest, &backup)?;
        assert_eq!(tread(dest.join("2026-06.toml"))?, "new");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn claim_existing_pending_returns_empty_when_absent() -> crate::Result<()> {
        // 既存 `pending-summary` が無ければ初回 publish として空内容を返す。
        let dir = temp_dir("claim-absent")?;
        let path = dir.join("pending-summary");
        let claim = dir.join("pending-summary.appending");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&claim);
        assert!(claim_existing_pending(&path, &claim)?.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn claim_existing_pending_reads_and_owns_existing() -> crate::Result<()> {
        // 既存 `pending-summary` を claim へ rename して内容を返し、live ファイルは claim 側へ移る。
        let dir = temp_dir("claim-read")?;
        let path = dir.join("pending-summary");
        let claim = dir.join("pending-summary.appending");
        let _ = std::fs::remove_file(&claim);
        twrite(&path, "block-A\n")?;
        assert_eq!(claim_existing_pending(&path, &claim)?, b"block-A\n");
        assert!(!path.exists());
        assert_eq!(tread(&claim)?, "block-A\n");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn claim_existing_pending_restores_pending_when_read_fails() -> crate::Result<()> {
        // read 失敗経路でも `pending-summary` を失わない。live が directory のとき rename(live→claim) は成功し
        // 直後の read(claim) が「Is a directory」で失敗するため、read 失敗→claim を live へ戻す経路を踏む。
        // 復元後に `pending-summary` が元の位置へ戻っており、consumer から消失しないことを固定する。
        let dir = temp_dir("claim-read-fail")?;
        let path = dir.join("pending-summary");
        let claim = dir.join("pending-summary.appending");
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::remove_dir_all(&claim);
        std::fs::create_dir_all(&path).map_err(|error| anyhow!("mkdir live: {error}"))?;
        twrite(path.join("marker"), "kept")?;

        let result = claim_existing_pending(&path, &claim);
        assert!(result.is_err());
        // claim は live へ戻り、内容（marker）も保持されている。
        assert!(path.is_dir());
        assert!(!claim.exists());
        assert_eq!(tread(path.join("marker"))?, "kept");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn assert_not_symlink_rejects_symlink_allows_regular_and_absent() -> crate::Result<()> {
        let dir = temp_dir("symlink-check")?;
        // 通常ファイルは許可。
        let regular = dir.join("flake.lock");
        twrite(&regular, "{}\n")?;
        assert!(assert_not_symlink(&regular).is_ok());
        // 不在は許容。
        let absent = dir.join("flake.nix");
        let _ = std::fs::remove_file(&absent);
        assert!(assert_not_symlink(&absent).is_ok());
        // symlink は拒否（root 権限でのリンク先上書き＝権限昇格を防ぐ）。
        let link = dir.join("link.lock");
        let target = dir.join("target");
        twrite(&target, "secret")?;
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&target, &link).map_err(|error| anyhow!("symlink: {error}"))?;
        assert!(assert_not_symlink(&link).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}

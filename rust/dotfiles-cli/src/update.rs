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
//! ## 適用要否判定は lock 更新「後」に行う（fleet 追随の根幹）
//!
//! ローカル flake の `flake.lock` における dotfiles pin は、`nix flake update dotfiles` を実行するまで前回適用
//! 値のまま動かない。upstream（dotfiles repo）が nightly bump で進んでも、ローカル lock を更新しない限り
//! ローカル pin は古いままである。よって「ローカル pin == `last-applied-rev` なら skip」を **lock 更新前**に
//! 判定すると、定常状態で常に skip され、マシンは新しい repo pin を永久に発見できず fleet が nightly bump に
//! 追随しない。これを避けるため、本 module は flock 取得後に **先に `nix flake update dotfiles` を実行して
//! ローカル lock を最新 repo pin へ更新し**、更新後の pin を読んで `last-applied-rev` と比較する。pin が変化
//! していなければ switch / record / marker を skip し（lock 更新は冪等で副作用が小さい）、変化していれば switch
//! と要約と marker 更新を行う。catch-up 要約に使う old nixpkgs rev は **lock 更新前**に、new は更新後に読む。
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

use crate::{
    Result, local_flake, process::run as run_process, process::run_capture, switch, update_history,
};

/// state dir 配下のファイル名（いずれもユーザ所有で作る）。
const LAST_APPLIED_REV: &str = "last-applied-rev";
const PENDING_SUMMARY: &str = "pending-summary";
const LAST_RUN_LOG: &str = "last-run.log";
const LOCK_FILE: &str = "update.lock";

/// 適用済み dotfiles flake input source 内の更新履歴ディレクトリ（`<source>/docs/update-history`）。
const HISTORY_SUBDIR: &str = "docs/update-history";

/// state dir 配下に複製した更新履歴のローカル複製先（`<state-dir>/history`）。
///
/// `show` / 適用後要約はこのローカル複製を読む。複製元は適用済み dotfiles flake input の source
/// （`~/.config/dotfiles` のローカル flake が pin する store path）の `docs/update-history` である。
/// ローカル `~/.config/dotfiles` 自身には `docs/update-history` が無い（init が作るのは flake.nix/flake.lock
/// だけ）ため、config dir を直接読むと show が常に空になる。適用時に input source からここへ複製し、
/// 以降の読取りは offline・決定論でこのローカル複製を参照する。
const HISTORY_LOCAL_SUBDIR: &str = "history";

/// auto 経路の入口。**先に lock を更新してから** repo pin を読み、前回適用済み rev と異なるときだけ適用する。
///
/// 順序: state dir 確保 → `update.lock` 非ブロッキング取得 → （取得失敗なら skip）→ lock 保持下で **適用前の
/// 推移的 nixpkgs old rev を解決**（lock 更新前の値）→ `nix flake update dotfiles` で**ローカル lock を最新 repo
/// pin へ更新**（`--commit-rev-marker` では lock 更新も switch もしない）→ 更新後の dotfiles pin を読む →
/// `last-applied-rev` と比較 → 同一なら switch / record / marker を skip（lock 更新は冪等で副作用小）→ 異なれば
/// switch → `last-applied-rev` を原子的更新 → **適用前 nixpkgs rev 起点**の catch-up 要約を tty/非 tty で振り分け
/// 表示。skip 判定を lock 更新「後」に置くのは、ローカル pin が `nix flake update dotfiles` 前は前回適用値の
/// まま動かず、更新前判定だと定常状態で常に skip して fleet が nightly bump へ追随しなくなるためである。要約
/// 選択は `nixpkgs_old` と突合するため、dedup 用 dotfiles pin ではなく nixpkgs old rev を起点に渡す（名前空間が
/// 異なるため pin SHA を渡すと span が恒久空になる）。lock は処理終端で解放する（guard の drop）。`--dry-run`
/// では実際の lock 更新・適用・状態書込みをせず、判定・表示経路だけを通す。
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

    // `--commit-rev-marker`: 二段適用（home→darwin）の **両成功後** に rev マーカーだけを確定させる経路。
    // home/darwin が別 CLI 起動に分かれる daemon ラッパーで、home ステップが既に lock を更新済みのため、
    // ここでは lock 更新も switch もせず、更新後の現在 pin を `last-applied-rev` へ原子的に書くだけ（lock 区間内）。
    if options.commit_rev_marker {
        let current_pin = read_repo_pin(&config_dir)?;
        write_last_applied_rev(&state_dir, &current_pin, dry_run)?;
        println!("適用済み rev を確定しました（rev {current_pin}）");
        return Ok(());
    }

    // lock 更新「前」に config flake.lock の推移的 nixpkgs rev（old nixpkgs rev）を解決する。これが catch-up
    // 要約の選択起点になる。lock 更新後に読むと nixpkgs も bump されて起点が new 側へずれ span が空になりうる
    // ため、必ず更新前に読む。`last-applied-rev`（dotfiles pin）は適用要否 dedup 専用であり、要約選択には使わない
    // （要約は `nixpkgs_old` と突合するため、dotfiles pin SHA を渡すと名前空間が違い恒久 miss する）。
    let previous_nixpkgs_rev = read_nixpkgs_rev(&config_dir)?;

    // **先に** ローカル lock を最新 repo pin へ更新する（skip 判定はこの後）。lock 更新前のローカル pin は前回
    // 適用値のまま動かないため、更新前に判定すると定常状態で常に skip し fleet が追随しない。lock 更新は冪等で
    // 副作用が小さいので、skip ケースでも先に走らせて upstream の新 pin を発見させる。
    update_lock(&config_dir, options.full, dry_run)?;

    // lock 更新後の dotfiles pin を読む。これが今回の適用対象（upstream の最新 repo pin）。
    let current_pin = read_repo_pin(&config_dir)?;

    let previous_rev = read_last_applied_rev(&state_dir)?;
    if !should_switch(previous_rev.as_deref(), &current_pin) {
        // lock 更新後の pin が前回適用済みと同一。switch / record / marker を skip する（lock 更新は実施済み）。
        println!("適用済み pin と同一のため switch は不要です（rev {current_pin}）");
        return Ok(());
    }

    // 更新後 pin が前回と異なる → switch を実行する（lock 更新は上で済んでいる）。
    switch::run(options.switch.clone())?;

    // 適用済み dotfiles flake input source の `docs/update-history` を state dir のローカル複製へ取り込む。
    // `~/.config/dotfiles` 自身には更新履歴が無いため、input source（pin が指す store path）から複製し、
    // 以降の要約・show をこのローカル複製から offline・決定論で読む。複製失敗（network 無し・解決不能・
    // read-only source 等）は適用を止めず、既存の複製があればそれを使う graceful degradation にする。
    // `--dry-run` では複製しない。
    if let Err(error) = sync_history(&config_dir, &state_dir, dry_run) {
        // best-effort: 履歴複製失敗は警告に留め、switch/適用は続行する（履歴は補助情報）。
        eprintln!("更新履歴の複製に失敗しました（既存の履歴複製を使用します）: {error}");
    }

    // `--defer-rev-marker`: home/darwin を別ステップで適用する daemon ラッパー向けに、rev マーカー書込みを
    // ここでは行わず、darwin 成功後の `--commit-rev-marker` 起動へ委ねる。これにより darwin 失敗時に rev が
    // 適用済みと誤記録されて次回 skip し drift する（darwin 未収束のまま放置）問題を防ぐ。defer 時も適用後
    // 要約は表示する（home 適用は実際に進んでいるため）。
    if !options.defer_rev_marker {
        write_last_applied_rev(&state_dir, &current_pin, dry_run)?;
    }
    present_summary(&state_dir, Some(previous_nixpkgs_rev.as_str()), dry_run)?;
    Ok(())
}

/// lock 更新「後」の現在 pin と前回適用済み rev から、switch / record / marker を実行すべきかを決める純粋関数。
///
/// 前回適用済み rev が無い（初回）か、lock 更新で得た現在 pin と異なるときに `true`（switch すべき）を返す。
/// 同一なら `false`（skip）。この判定は **必ず `nix flake update dotfiles` 実行後の pin** に対して行うことが
/// 機能の根幹で、更新前のローカル pin（前回適用値のまま動かない）に対して判定すると定常状態で常に skip し、
/// マシンが upstream の新 repo pin を永久に発見できず fleet が nightly bump に追随しなくなる。判定を実行・I/O
/// から切り離し、「更新前 pin == 前回値でも、更新後 pin が新 pin なら switch する／更新後 pin が前回と同一なら
/// skip する」という根幹挙動を単体検証可能にする。
fn should_switch(previous_rev: Option<&str>, current_pin: &str) -> bool {
    previous_rev != Some(current_pin)
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

/// 適用済み dotfiles flake input source の `docs/update-history` を state dir のローカル複製へ取り込む。
///
/// `~/.config/dotfiles` のローカル flake は `flake.nix`/`flake.lock` だけを持ち更新履歴を含まないため、
/// 履歴は **適用済み dotfiles input が指す store path**（`flake.lock` の dotfiles input の locked rev に対応
/// する source）から複製する。複製先は `<state-dir>/history` で、以降の show/要約はこのローカル複製を
/// offline・決定論で読む。複製は「source path 解決 → `<source>/docs/update-history` を `<state-dir>/history`
/// へコピー」の 2 段で、source path 解決（`nix flake metadata`）や copy が失敗しても record/適用は止めず、
/// 既存複製があればそれを使う graceful degradation にする（履歴は補助情報であり適用の前提ではない）。
/// `--dry-run` では複製しない。
fn sync_history(config_dir: &Path, state_dir: &Path, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    // source path を解決できない（network 無し・metadata 失敗）場合は既存複製を温存して終了する。
    let Some(source_root) = resolve_input_source(config_dir) else {
        return Ok(());
    };
    let source_history = source_root.join(HISTORY_SUBDIR);
    if !source_history.is_dir() {
        // source 側に履歴 dir が無ければ複製対象が無い。既存複製を温存する。
        return Ok(());
    }
    let dest = state_dir.join(HISTORY_LOCAL_SUBDIR);
    copy_history_dir(&source_history, &dest)
}

/// 適用済み dotfiles flake input の source store path を解決する（解決不能なら `None`）。
///
/// `nix flake metadata <config-dir> --json` の `locks.nodes.<dotfiles>.locked` が指す store path を返す。
/// network 無し・nix 不在・metadata 失敗・JSON 解析失敗はいずれも `None` へ縮退し、呼び出し側で既存複製の
/// 温存に倒す（履歴複製は best-effort で、解決失敗を致命にしない）。
fn resolve_input_source(config_dir: &Path) -> Option<PathBuf> {
    let args = [
        OsString::from("flake"),
        OsString::from("metadata"),
        config_dir.as_os_str().to_os_string(),
        OsString::from("--json"),
    ];
    let json = run_capture("nix", args).ok()?;
    parse_input_source_path(&json, local_flake::INPUT_NAME).map(PathBuf::from)
}

/// `nix flake metadata --json` 出力から指定 input の locked source store path を抽出する純粋関数。
///
/// `locks.nodes.<input>.locked.path`（path 系 source）または、store path を直接持つ `locked` 表現から
/// store path を取り出す。抽出を実行から切り離し、metadata JSON 構造の解釈を単体検証できるようにする。
/// path が無い形式（解決前など）は `None` を返す。
fn parse_input_source_path(metadata_json: &str, input_name: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(metadata_json).ok()?;
    let locked = value
        .get("locks")
        .and_then(|locks| locks.get("nodes"))
        .and_then(|nodes| nodes.get(input_name))
        .and_then(|node| node.get("locked"))?;
    locked
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// `<source>/docs/update-history` のディレクトリ内 `*.toml` を `<state-dir>/history` へ複製する。
///
/// 複製先を作り直し（既存複製を新 source の内容で置き換え）、source 直下の通常ファイルだけを名前ごとコピー
/// する（サブディレクトリは履歴 layout 上想定しないため対象外）。store path 由来 source は read-only な
/// ため、複製先はユーザ所有 state dir に置いて以降の読取りを保証する。コピー失敗は呼び出し側が best-effort
/// として扱えるよう `Err` を返すが、致命にしないのは呼び出し側の責務である。
fn copy_history_dir(source_history: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)
        .with_context(|| format!("failed to create history dir {}", dest.display()))?;
    for entry in fs::read_dir(source_history)
        .with_context(|| format!("failed to read {}", source_history.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let from = entry.path();
        let to = dest.join(entry.file_name());
        fs::copy(&from, &to)
            .with_context(|| format!("failed to copy {} to {}", from.display(), to.display()))?;
    }
    Ok(())
}

/// 更新履歴のローカル複製ディレクトリ（`<state-dir>/history`）を返す。
///
/// `update-history show`（`--source` 未指定時）が読む既定 source。適用時に dotfiles input source から
/// この dir へ複製された履歴を offline・決定論で読むための共有解決点であり、`update` 経路と `update-history`
/// 経路で同一の state dir 解決規則（XDG/HOME）を使う。
pub(crate) fn history_local_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join(HISTORY_LOCAL_SUBDIR))
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
fn present_summary(state_dir: &Path, nixpkgs_from_rev: Option<&str>, dry_run: bool) -> Result<()> {
    // 履歴は state dir のローカル複製（`<state-dir>/history`）から読む。`~/.config/dotfiles` には更新履歴が
    // 無く、適用時に input source から複製済みのこの dir を offline・決定論で参照する。
    let source = state_dir.join(HISTORY_LOCAL_SUBDIR);

    if std::io::stdout().is_terminal() {
        // tty: 起動元端末へ直接表示。stdout を sink にして show 描画を再利用する。
        update_history::render_applied_summary(&source, nixpkgs_from_rev, std::io::stdout())?;
        if !dry_run {
            append_last_run_log(state_dir, nixpkgs_from_rev)?;
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
    append_last_run_log(state_dir, nixpkgs_from_rev)?;
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
/// 適用の人間可読な要約を上書き保存し、直近 1 回分の適用内容を後から確認できるようにする。履歴 source は
/// state dir のローカル複製（`<state-dir>/history`）を読む。
fn append_last_run_log(state_dir: &Path, nixpkgs_from_rev: Option<&str>) -> Result<()> {
    let path = state_dir.join(LAST_RUN_LOG);
    let source = state_dir.join(HISTORY_LOCAL_SUBDIR);
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
        LAST_RUN_LOG, LOCK_FILE, PENDING_SUMMARY, UpdateLock, append_pending_summary,
        copy_history_dir, parse_input_source_path, parse_nixpkgs_rev, parse_repo_pin,
        present_summary, read_last_applied_rev, resolve_state_dir, should_switch, update_args,
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
    fn skip_decision_is_made_against_post_update_pin() {
        // P1-1 退行固定: skip 判定は `nix flake update dotfiles` 実行「後」の pin に対して行う。
        //
        // 定常状態ではローカル lock の dotfiles pin は前回適用値（"old"）のまま動かない。run() は lock 更新後の
        // pin を read_repo_pin で読み直してから本関数へ渡すため、以下の 2 ケースを固定する:
        //   1. 前回適用値 = "old"。lock 更新で pin が新値 "new" になれば switch する（追随する）。
        //      更新前の pin "old" を渡していた旧実装ではここが常に skip で fleet が追随しなかった。
        assert!(
            should_switch(Some("old"), "new"),
            "lock 更新後に新 pin になれば skip せず switch する"
        );
        //   2. lock 更新後の pin が前回適用値 "same" と同一なら switch を skip する（lock 更新は冪等で実施済み）。
        assert!(
            !should_switch(Some("same"), "same"),
            "更新後 pin が前回と同一なら switch を skip する"
        );
        // 初回（last-applied-rev 不在）は必ず switch する。
        assert!(should_switch(None, "first"), "初回は必ず switch する");
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

    /// テスト用に state dir のローカル履歴複製 `<state-dir>/history/2026-06.toml` を書く。
    ///
    /// `present_summary` は config dir の `docs/update-history` ではなく state dir のローカル複製
    /// （[`super::HISTORY_LOCAL_SUBDIR`]）を読むため、テストもそこへ履歴を置く。各エントリは
    /// `nixpkgs_old -> nixpkgs_new` のチェーンで、宣言アプリ 1 件を持つ（show 既定の宣言フィルタを通す）。
    fn write_history(state_dir: &Path, chain: &[(&str, &str)]) {
        let history_dir = state_dir.join(super::HISTORY_LOCAL_SUBDIR);
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
        present_summary(&dir, Some("nA"), false)?;
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
        present_summary(&dir2, Some("dotfilespin-not-a-nixpkgs-rev"), false)?;
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
    fn present_summary_dry_run_has_no_file_side_effect() -> crate::Result<()> {
        // dry-run 契約: 非 tty 経路でも `pending-summary` / `last-run.log` を書かない（副作用抑止）。
        // 既存 `dry_run_lock_has_no_file_side_effect` と同じく、dry_run=true で実ファイルが生成されないことを
        // assert する。テスト環境は非 tty のため present_summary は background 分岐を通り、dry_run=false なら
        // 両ファイルへ書く（present_summary_selects_span_by_nixpkgs_rev_not_dotfiles_pin で固定済み）。
        let dir = temp_dir("present-dry");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create state+config dir");
        write_history(&dir, &[("nA", "nB"), ("nB", "nC")]);

        present_summary(&dir, Some("nA"), true)?;
        assert!(
            !dir.join(PENDING_SUMMARY).exists(),
            "dry-run must not write pending-summary"
        );
        assert!(
            !dir.join(LAST_RUN_LOG).exists(),
            "dry-run must not write last-run.log"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn append_pending_summary_accumulates_rev_blocks() -> crate::Result<()> {
        // 非 tty 連続適用で pending-summary が rev 単位に**追記**累積し、未表示 rev を失わないことを実ファイルで固定。
        let dir = temp_dir("pending-accumulate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        write_history(&dir, &[("r1", "r2"), ("r2", "r3")]);
        let source = dir.join(super::HISTORY_LOCAL_SUBDIR);

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

    #[test]
    fn parse_input_source_path_reads_dotfiles_locked_path() {
        // N3 退行固定: `nix flake metadata --json` の locks.nodes.dotfiles.locked.path（store path）を解決する。
        // 履歴複製元はこの input source path（適用済み pin の source）であり、config dir 直下ではない。
        let metadata = r#"{
          "locks": {
            "nodes": {
              "dotfiles": {
                "locked": { "path": "/nix/store/abc-dotfiles-source", "type": "path" }
              },
              "root": { "inputs": { "dotfiles": "dotfiles" } }
            },
            "root": "root",
            "version": 7
          },
          "path": "/nix/store/zzz-config-flake"
        }"#;
        assert_eq!(
            parse_input_source_path(metadata, "dotfiles").as_deref(),
            Some("/nix/store/abc-dotfiles-source")
        );
        // path を持たない形式（解決前 / 別 source 種別）は None へ縮退する（既存複製温存に倒す）。
        let no_path = r#"{ "locks": { "nodes": { "dotfiles": { "locked": { "rev": "x" } } } } }"#;
        assert!(parse_input_source_path(no_path, "dotfiles").is_none());
        // 壊れた JSON も None。
        assert!(parse_input_source_path("not json", "dotfiles").is_none());
    }

    #[test]
    fn copy_history_dir_replicates_toml_files_to_local_dest() -> crate::Result<()> {
        // N3 退行固定: input source の docs/update-history 配下の通常ファイルを state dir のローカル複製へ
        // コピーする。複製先は読取り対象（show/要約）になる。
        let base = temp_dir("copy-history");
        let _ = std::fs::remove_dir_all(&base);
        let source_history = base.join("src-docs-history");
        std::fs::create_dir_all(&source_history).expect("create source history");
        std::fs::write(source_history.join("2026-05.toml"), "a = 1\n").expect("write src toml1");
        std::fs::write(source_history.join("2026-06.toml"), "b = 2\n").expect("write src toml2");
        // サブディレクトリは複製対象外（履歴 layout 上想定しない）。
        std::fs::create_dir_all(source_history.join("nested")).expect("create nested");

        let dest = base.join("history");
        copy_history_dir(&source_history, &dest)?;

        assert_eq!(
            std::fs::read_to_string(dest.join("2026-05.toml")).expect("read dest toml1"),
            "a = 1\n"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("2026-06.toml")).expect("read dest toml2"),
            "b = 2\n"
        );
        // サブディレクトリはコピーされない。
        assert!(!dest.join("nested").exists());

        let _ = std::fs::remove_dir_all(&base);
        Ok(())
    }
}

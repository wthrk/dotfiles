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
/// 最後に**適用に成功した**状態の推移的 nixpkgs rev（catch-up 要約 span の真の起点）。
///
/// 要約 span の起点に「lock 更新前のローカル lock の nixpkgs rev」を使うと、partial-failure 再実行で誤る:
/// 前回実行が `nix flake update` まで進んで lock を新 pin へ bump した後に switch/darwin で失敗すると、
/// `last-applied-rev` は古いまま・ローカル lock は新 pin になる。次回実行で lock 更新「前」に nixpkgs rev を
/// 読んでも、その値は既に bump 済みの新 rev であり、要約の old が new pin と一致して差分が消える。これを避け、
/// **最後に適用成功した時点の nixpkgs rev** をこのファイルへ確定書込みし、要約 span の起点に使う（未適用範囲の
/// 実起点を指す）。`last-applied-rev` と同時に書き、未確定（defer）時は書かない。
const LAST_APPLIED_NIXPKGS_REV: &str = "last-applied-nixpkgs-rev";
/// 最後に**要約を表示/追記し終えた**範囲の終端（new 側）nixpkgs rev（catch-up 要約 span の起点に使う）。
///
/// catch-up 要約 span の起点は「適用済み rev」ではなく「**最後に利用者へ見せ終えた rev**」でなければ
/// show-once が壊れる。`last-applied-nixpkgs-rev` は適用成功時にしか確定せず（defer 経路では書かれない）、
/// これを span 起点に使うと、同日に shell catch-up（defer）と daemon home（defer）が両方走る通常ケースで
/// 二度見えする: shell catch-up は span 起点 = N0 で N0→N1 を追記（defer なので applied 未更新）、daemon home
/// も span 起点が依然 N0（defer で applied 未更新）のため **同一 N0→N1 を再追記** してしまう。
///
/// これを防ぐため、**要約を append/表示し終えた直後**に、その範囲の new 側 nixpkgs rev をこの marker へ
/// 確定書込みし（defer 経路でも commit 経路でも書く）、次回 present_summary はこの marker を span 起点に読む。
/// 2 回目は起点 = N1 → `select_entries` が `nixpkgs_old == N1` を見つけられず空 → 再追記しない（A: 二重抑止）。
/// partial-failure では switch 失敗時に要約自体が走らずこの marker も進まないため、前回要約済み rev が保たれ、
/// 再実行で未表示範囲を失わない（B: partial-failure 堅牢性）。要約「後」に書く点が `last-applied-*` と異なる。
const LAST_SUMMARIZED_NIXPKGS_REV: &str = "last-summarized-nixpkgs-rev";
const PENDING_SUMMARY: &str = "pending-summary";
const LAST_RUN_LOG: &str = "last-run.log";
const LOCK_FILE: &str = "update.lock";
/// `--defer-rev-marker` 適用時に **その時点で適用した** dotfiles repo pin を控える state file（ユーザ所有）。
///
/// daemon ラッパーは home 適用後に user 側 `update.lock` を解放してから root の `darwin-rebuild` →
/// `--commit-rev-marker` を別 CLI 起動で実行する。`--commit-rev-marker` が「現在の repo pin」を読み直して
/// 確定すると、home 適用時点の pin と darwin commit 時点の pin が乖離した場合に **適用していない pin を
/// `last-applied` へ確定**しうる（commit までの間に lock が再 bump される競合）。これを避けるため、defer 時に
/// 適用した pin をこのファイルへ控え、commit はこの defer 値を読んで確定する（commit 時に現在 pin を読み直さ
/// ない）。defer 値が無い場合（defer を経ない直接 commit 呼び出し）だけ後方互換で現在 pin へフォールバックする。
const DEFERRED_REV: &str = "deferred-rev";
/// `--defer-rev-marker` 適用時に控える、適用時点の推移的 nixpkgs rev（`DEFERRED_REV` と対）。
///
/// `last-applied-nixpkgs-rev` を commit で確定する際、dotfiles pin と同様に defer 時点の値を使い、commit 時の
/// 現在 nixpkgs rev を読み直さない。これにより dotfiles pin / nixpkgs rev の両方が defer 時点で整合する。
const DEFERRED_NIXPKGS_REV: &str = "deferred-nixpkgs-rev";

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
    // ここでは lock 更新も switch もしない。
    //
    // 確定する pin は **home defer ステップで実際に適用した pin**（`deferred-rev`）を読む（B）。commit 時に
    // 「現在の repo pin」を読み直すと、home 適用後・commit 前に lock が再 bump された場合に **適用していない
    // pin を `last-applied` へ確定**しうる。defer 時に控えた pin を確定すれば、適用した pin と確定する pin が
    // 必ず一致する。defer 値が無い場合（defer を経ない直接 commit 呼び出し）だけ後方互換で現在 pin へ縮退する。
    if options.commit_rev_marker {
        let committed_pin = match read_deferred_rev(&state_dir)? {
            Some(rev) => rev,
            None => read_repo_pin(&config_dir)?,
        };
        write_last_applied_rev(&state_dir, &committed_pin, dry_run)?;
        // dotfiles pin と同時に、適用時点の推移的 nixpkgs rev も確定する。これも defer 時点で控えた値を優先し、
        // 無ければ現在値へ縮退する（dotfiles pin と同じ defer 時点の整合を保つ）。これは要約 span 起点解決の
        // 二次フォールバック（最優先は要約後に進む `last-summarized-nixpkgs-rev`。home defer ステップが要約済み）。
        let committed_nixpkgs_rev = match read_deferred_nixpkgs_rev(&state_dir)? {
            Some(rev) => rev,
            None => read_nixpkgs_rev(&config_dir)?,
        };
        write_last_applied_nixpkgs_rev(&state_dir, &committed_nixpkgs_rev, dry_run)?;
        // 確定後は defer marker を消す（次回 defer→commit サイクルへ古い値を持ち越さない）。dry-run では触らない。
        clear_deferred_markers(&state_dir, dry_run);
        println!("適用済み rev を確定しました（rev {committed_pin}）");
        return Ok(());
    }

    // 新しい適用サイクルの開始時に、前サイクルの deferred marker（`deferred-rev`/`deferred-nixpkgs-rev`）を
    // 消してサイクルローカル化する（安全性質を状態機械内へ閉じる）。
    //
    // commit 経路（上で return 済み）は確定後に marker を消すが、それは「defer→commit が必ず対で走る」現行
    // ラッパーの不変条件に依存している。darwin 失敗（auto-update.nix の `set -e`）で commit へ到達せず deferred
    // marker が残骸化すると、将来 home/darwin/commit を別ジョブへ分離した場合に、後続サイクルの commit が **この
    // サイクルで適用していない古い defer 値を `last-applied` へ誤確定**しうる。これを wrapper の結線に依存せず
    // 防ぐため、**defer を実際に書く前（新サイクルの冒頭）で既存 deferred marker を必ずクリア**する。これにより
    // commit が読む deferred marker は「このサイクルの defer ステップが書いた値」だけになり、前サイクルの残骸を
    // 確定しない（残骸はこのクリアで消えるため、defer を経ない commit は現在 pin への後方互換縮退に倒れる）。
    // dry-run では状態を触らない。
    clear_deferred_markers(&state_dir, dry_run);

    // catch-up 要約 span の起点となる「最後に利用者へ見せ終えた nixpkgs rev」を解決する。
    //
    // 最優先は **最後に要約を表示/追記し終えた範囲の new 側 nixpkgs rev**（`last-summarized-nixpkgs-rev`）。
    // これを起点に使う理由は二つある:
    //   (A) 同日二重追記の抑止: shell catch-up（defer）と daemon home（defer）が同日に両方走っても、先行
    //       実行が要約後にこの marker を N1 へ進めるため、後続は起点 = N1 → `select_entries` が空 → 再追記
    //       しない（`last-applied-nixpkgs-rev` は defer で更新されず、起点に使うと同一ブロックを二度追記する）。
    //   (B) partial-failure 堅牢性: switch/darwin が要約「前」に失敗するとこの marker は進まないため、前回
    //       要約済み rev が保たれ、再実行で未表示範囲（要約 span）を失わない。
    // marker が無いとき（このコード導入前に適用済み・本当の初回）は、適用要否 dedup 専用の値ではなく要約用の
    // 値へ縮退する: まず `last-applied-nixpkgs-rev`（旧経路で commit 時に書かれた最後の適用 rev）、それも無ければ
    // lock 更新「前」のローカル lock の nixpkgs rev（適用済み状態の推定起点）へフォールバックする。`last-applied-rev`
    // （dotfiles pin）は適用要否 dedup 専用で要約選択には使わない（要約は `nixpkgs_old` と突合するため、dotfiles
    // pin SHA を渡すと名前空間が違い恒久 miss する）。
    let span_start_nixpkgs_rev = match read_last_summarized_nixpkgs_rev(&state_dir)? {
        Some(rev) => rev,
        None => match read_last_applied_nixpkgs_rev(&state_dir)? {
            Some(rev) => rev,
            None => read_nixpkgs_rev(&config_dir)?,
        },
    };

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
    //
    // ただし複製が失敗した場合は **要約表示と要約済み marker の確定を skip** する（A）。sync_history が失敗
    // すると初回や一時的な archive 失敗で `<state>/history` 複製が無いまま present_summary が空/古い履歴を
    // 読み、それでも `last-summarized-nixpkgs-rev` を new rev へ進めると、その rev の要約が永久に失われる
    // （次回は marker が先へ進んでいて再表示されない）。よって履歴が読めた時だけ summarized を進め、複製失敗
    // 時は span 起点を保って次回再試行で表示できるようにする。適用 dedup の `last-applied-*` 確定とは分離し、
    // last-applied は履歴複製の成否に依らず確定する（適用自体は進んでいるため）。
    let history_synced = match sync_history(&config_dir, &state_dir, dry_run) {
        Ok(()) => true,
        Err(error) => {
            // best-effort: 履歴複製失敗は警告に留め、switch/適用は続行する（履歴は補助情報）。
            eprintln!(
                "更新履歴の複製に失敗しました（要約表示と要約済み rev の確定は次回へ繰り越します）: {error}"
            );
            false
        }
    };

    // `--defer-rev-marker`: home/darwin を別ステップで適用する daemon ラッパー向けに、rev マーカー書込みを
    // ここでは行わず、darwin 成功後の `--commit-rev-marker` 起動へ委ねる。これにより darwin 失敗時に rev が
    // 適用済みと誤記録されて次回 skip し drift する（darwin 未収束のまま放置）問題を防ぐ。defer 時も適用後
    // 要約は表示する（home 適用は実際に進んでいるため）。
    // 今回適用した new 側 nixpkgs rev（lock は上の `update_lock` で new pin へ更新済み）。要約 span の終端
    // （次回起点）であり、適用要否 marker（`last-applied-*`）とも整合する。
    let applied_nixpkgs_rev = read_nixpkgs_rev(&config_dir)?;

    // repo pin 全体の確定（`last-applied-rev`/`last-applied-nixpkgs-rev`）は **全体適用（target=all）でのみ**
    // 行う。部分 target（`dotfiles update home` / `dotfiles update darwin`）の通常実行でこれを確定すると、
    // 適用していない他 target がその rev について以降 skip され（`should_switch` が前回値一致で skip）、未適用の
    // まま starve する。部分 target では rev を確定せず次回の全体適用に残す。daemon 経路の二段適用は
    // `--defer-rev-marker`（home 部分で確定しない）/`--commit-rev-marker`（darwin 成功後にまとめて確定）で
    // 整合させており、ここでは `defer_rev_marker` 偽かつ全体適用のときだけ確定する。要約表示自体は target に
    // 依らず行う（部分適用でも実際に進んだ範囲を見せる）が、apply-dedup の rev 確定は全体適用に限定する。
    if !options.defer_rev_marker && options.switch.is_full_apply() {
        write_last_applied_rev(&state_dir, &current_pin, dry_run)?;
        // dotfiles pin と同時に、今回適用した nixpkgs rev も確定する。defer 時は rev 未確定のため書かない
        // （darwin 成功後の `--commit-rev-marker` がまとめて確定する）。
        write_last_applied_nixpkgs_rev(&state_dir, &applied_nixpkgs_rev, dry_run)?;
    } else if options.defer_rev_marker {
        // defer 経路: `last-applied-*` はまだ確定しないが、**この時点で適用した pin / nixpkgs rev** を defer
        // marker へ控える（B）。後続の `--commit-rev-marker` はこの defer 値を確定し、commit 時に現在 pin を
        // 読み直さない。これにより home 適用後・commit 前に lock が再 bump されても、適用した pin と確定する pin
        // が必ず一致し、適用していない pin を `last-applied` へ確定する乖離を防ぐ。
        write_deferred_rev(&state_dir, &current_pin, dry_run)?;
        write_deferred_nixpkgs_rev(&state_dir, &applied_nixpkgs_rev, dry_run)?;
    }

    // 要約を表示/追記する。**この後で** `last-summarized-nixpkgs-rev` を進めるのが要点で、要約「前」に
    // marker を進めると partial-failure（switch 後・要約前に異常終了）で未表示範囲を失う。逆に要約「後」に
    // 進めることで、同日に defer 経路が連続しても 2 回目は起点 = new rev → 空 span → 再追記しない。
    //
    // 履歴複製が失敗（`history_synced == false`）したときは要約も marker 確定も行わない（A）。複製が無いまま
    // 空/古い履歴で要約すると、見せていない rev について marker だけ進み、その rev の要約が永久に失われる。
    // 複製が成功した時だけ要約 → marker 確定へ進み、失敗時は span 起点を保って次回再試行に委ねる。
    //
    // tty 判定はアンビエント大域（`std::io::stdout().is_terminal()`）への依存を呼び出し元へ集約するため、
    // ここで 1 回だけ解決して `present_summary` へ bool で注入する。`present_summary` 内で is_terminal() を
    // 呼ぶと、stdout が tty になる環境（nix build sandbox の builder）でテストが tty 経路へ入り pending 未書込み
    // → NotFound で壊れる（非 hermetic）。注入化で分岐が大域から切れ、テストは bool を渡して決定論的に経路を
    // exercise でき、production の挙動（tty なら端末描画、非 tty なら pending 追記）は不変。
    if history_synced {
        let stdout_is_terminal = std::io::stdout().is_terminal();
        present_summary(
            &state_dir,
            Some(span_start_nixpkgs_rev.as_str()),
            dry_run,
            stdout_is_terminal,
        )?;

        // 要約済み範囲の終端を確定する。defer 経路でも commit 経路でも、要約を見せ終えた直後に必ず書く。
        // これが次回 present_summary の span 起点になり、同日二重追記と partial-failure 堅牢性を両立させる。
        write_last_summarized_nixpkgs_rev(&state_dir, &applied_nixpkgs_rev, dry_run)?;
    }
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
/// へコピー」の 2 段で、source path 解決（`nix flake archive --json`）や copy が失敗しても record/適用は止めず、
/// 既存複製があればそれを使う graceful degradation にする（履歴は補助情報であり適用の前提ではない）。
/// source path 解決は `nix flake archive --json`（github 型 input でも realize 済み store path を返す）に拠り、
/// `--dry-run` では複製しない。
fn sync_history(config_dir: &Path, state_dir: &Path, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    // source path を解決できない（network 無し・archive 失敗）場合は既存複製を温存して終了する。
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

/// 適用済み dotfiles flake input の **realize 済み source store path** を解決する（解決不能なら `None`）。
///
/// `nix flake archive <config-dir> --json --no-write-lock-file` の `inputs.<dotfiles>.path` が指す store path
/// を返す。`nix flake metadata --json` の `locks.nodes.<dotfiles>.locked` ではなく archive を使う理由は、
/// **本番の既定 source（`github:wthrk/dotfiles`、github 型 input）では metadata の locked ノードが `path` キーを
/// 持たない**（`owner/repo/rev/narHash/lastModified/type` のみ）ため、metadata からは github 型 input の store
/// path を取り出せず、本番経路で履歴複製が常に no-op になる（N3 が解消対象としたバグの再来）からである。
/// `nix flake archive --json` は input source を realize した実 store path を input 名ごとに返し、github 型でも
/// path 型でも `inputs.<input>.path` を持つため、両 source 型で確実に store path を得られる。`--no-write-lock-file`
/// は config flake の lock を書き換えない（既に `update_lock` で更新済み・読み取り専用にしたい）ため付ける。
/// switch 済みであれば input source は store に realize 済みなので、archive はクロージャの再コピーを伴わず
/// 既存 store path を報告するだけで軽量に済む。network 無し・nix 不在・archive 失敗・JSON 解析失敗はいずれも
/// `None` へ縮退し、呼び出し側で既存複製の温存に倒す（履歴複製は best-effort で、解決失敗を致命にしない）。
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
///
/// archive JSON は root flake の `inputs` map を持ち、各 input が `inputs`（推移依存）と `path`（realize 済み
/// store path）を持つ。`inputs.<input>.path` を取り出す。github 型・path 型のいずれの input source でも
/// `inputs.<input>.path` は realize 済み store path を指すため、source 種別に依らず抽出できる（github 型 input
/// に対して `path` を持たなかった metadata 由来パースの本番不全を塞ぐ）。抽出を実行から切り離し、archive JSON
/// 構造の解釈を単体検証できるようにする。`inputs.<input>` や `path` が無い形式は `None` を返す。
fn parse_input_source_path(archive_json: &str, input_name: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(archive_json).ok()?;
    value
        .get("inputs")
        .and_then(|inputs| inputs.get(input_name))
        .and_then(|node| node.get("path"))
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

/// 生成ローカル flake の `flake.lock` から dotfiles input の現在の repo pin（適用要否 dedup の同一性）を読む。
///
/// 既定の github source では `nodes.<INPUT_NAME>.locked.rev` を pin とする。これは各マシンが追随する dotfiles
/// repo の適用対象リビジョンであり、`last-applied-rev` との比較で適用要否を決める。
///
/// `dotfiles init --source path:/...` のローカル source（path 型 input）は `locked.rev` を持たない（F）。rev を
/// 必須にすると path source で `update` 経路が常に失敗する。そこで rev が無い場合は **`narHash`** を pin の
/// 同一性へフォールバックする（path source の内容が変われば narHash が動き、再適用される）。`narHash` も無ければ
/// `lastModified` へ縮退する。いずれも無い（pin 同一性を表す値が一切無い）場合だけ失敗にする（適用要否を誤判定
/// して未適用/重複適用に倒さないため）。github source はこのフォールバックに到達せず従来どおり rev で dedup する。
fn read_repo_pin(config_dir: &Path) -> Result<String> {
    let lock_path = config_dir.join("flake.lock");
    let text = fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    parse_repo_pin(&text, local_flake::INPUT_NAME)
        .with_context(|| format!("failed to resolve repo pin from {}", lock_path.display()))
}

/// `flake.lock` JSON テキストから指定 input の repo pin 同一性を抽出する純粋関数。
///
/// 抽出経路を実行から切り離し、lock JSON 構造の解釈を単体検証できるようにする。pin 同一性は `locked.rev`
/// （github source）→ `locked.narHash`（path source で rev が無い場合）→ `locked.lastModified`（数値も許容）の
/// 順で解決する。rev を持つ source は従来どおり rev で dedup し、rev の無い path source でも narHash/lastModified で
/// 適用要否を判定できるようにして update 経路が壊れないようにする（F）。いずれも無ければ `Err`。
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
///
/// rev（github source）を最優先し、rev の無い path source では narHash、それも無ければ lastModified を
/// 同一性に使う。lastModified は数値で現れうるため数値も文字列化して受ける。いずれも無ければ `None`。
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

/// 最後に適用成功した時点の nixpkgs rev を読む（不存在/空なら `None`）。
///
/// `None`（初回・未確定）なら呼び出し側は lock 更新前のローカル lock の nixpkgs rev へフォールバックする。
fn read_last_applied_nixpkgs_rev(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_APPLIED_NIXPKGS_REV))
}

/// 最後に要約を表示/追記し終えた範囲の new 側 nixpkgs rev を読む（不存在/空なら `None`）。
///
/// `None`（このコード導入前に適用済み・本当の初回）なら呼び出し側は `last-applied-nixpkgs-rev`、次いで
/// lock 更新前のローカル lock の nixpkgs rev へ縮退する。
fn read_last_summarized_nixpkgs_rev(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_SUMMARIZED_NIXPKGS_REV))
}

/// `last-applied-rev` を読む（不存在/空なら `None`）。
fn read_last_applied_rev(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_APPLIED_REV))
}

/// defer 時に控えた「適用した dotfiles repo pin」を読む（不存在/空なら `None`）。
///
/// `--commit-rev-marker` がこの値を最優先で確定する。`None`（defer を経ない直接 commit）なら現在 pin へ縮退する。
fn read_deferred_rev(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(DEFERRED_REV))
}

/// defer 時に控えた「適用した推移的 nixpkgs rev」を読む（不存在/空なら `None`）。
///
/// `DEFERRED_REV` と対で、commit 時に dotfiles pin と同じ defer 時点の値を確定するために使う。
fn read_deferred_nixpkgs_rev(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(DEFERRED_NIXPKGS_REV))
}

/// state file から trim 済み rev を読む（不存在/空は `None`、その他 I/O 失敗は文脈付き `Err`）。
fn read_trimmed_rev(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
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
    write_rev_atomic(&state_dir.join(LAST_APPLIED_REV), rev, dry_run)
}

/// 最後に適用成功した時点の nixpkgs rev を原子的に書き込む（ユーザ所有）。`--dry-run` では書かない。
///
/// `last-applied-rev` と同時に確定し、catch-up 要約 span の真の起点（未適用範囲の起点）として次回実行で読む。
fn write_last_applied_nixpkgs_rev(state_dir: &Path, rev: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(LAST_APPLIED_NIXPKGS_REV), rev, dry_run)
}

/// defer 時に適用した dotfiles repo pin を原子的に控える（ユーザ所有）。`--dry-run` では書かない。
///
/// 後続の `--commit-rev-marker` がこの defer 値を `last-applied-rev` へ確定するため、commit 時に現在 pin を
/// 読み直さず、適用した pin と確定する pin の乖離を防ぐ（B）。
fn write_deferred_rev(state_dir: &Path, rev: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(DEFERRED_REV), rev, dry_run)
}

/// defer 時に適用した推移的 nixpkgs rev を原子的に控える（ユーザ所有）。`--dry-run` では書かない。
///
/// `DEFERRED_REV` と対で、commit が `last-applied-nixpkgs-rev` を defer 時点の値で確定するために使う。
fn write_deferred_nixpkgs_rev(state_dir: &Path, rev: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(DEFERRED_NIXPKGS_REV), rev, dry_run)
}

/// deferred marker（`deferred-rev`/`deferred-nixpkgs-rev`）を消す。`--dry-run` では触らない。
///
/// 新サイクルの冒頭（defer 書込み前）と commit 確定後の両方で呼び、deferred 値を 1 サイクルへ閉じる。これにより
/// commit が読む deferred 値は **そのサイクルの defer ステップが書いた値だけ**になり、darwin 失敗等で commit へ
/// 到達しなかった前サイクルの残骸を、後続サイクルの commit が未適用 pin として誤確定しない（サイクルローカル化）。
/// 不存在の marker 除去は no-op（致命にしない）。
fn clear_deferred_markers(state_dir: &Path, dry_run: bool) {
    if dry_run {
        return;
    }
    let _ = fs::remove_file(state_dir.join(DEFERRED_REV));
    let _ = fs::remove_file(state_dir.join(DEFERRED_NIXPKGS_REV));
}

/// 最後に要約を表示/追記し終えた範囲の new 側 nixpkgs rev を原子的に書き込む（ユーザ所有）。`--dry-run` 不書込。
///
/// 要約「後」に書くことで、次回 present_summary の span 起点が「最後に見せ終えた rev」になり、同日二重追記の
/// 抑止（A）と partial-failure 堅牢性（B）を両立させる。
fn write_last_summarized_nixpkgs_rev(state_dir: &Path, rev: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(LAST_SUMMARIZED_NIXPKGS_REV), rev, dry_run)
}

/// rev を同一 dir 内 temp→rename で原子的に書く共有実装（部分書込みを観測させない）。
fn write_rev_atomic(final_path: &Path, rev: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    let file_name = final_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| LAST_APPLIED_REV.to_string());
    let temp_path = final_path.with_file_name(format!("{file_name}.{}.tmp", std::process::id()));
    fs::write(&temp_path, format!("{rev}\n"))
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, final_path)
        .with_context(|| format!("failed to atomically replace {}", final_path.display()))?;
    Ok(())
}

/// 適用後の要約を catch-up 集約し、tty なら stdout、非 tty なら `pending-summary` へ振り分ける。
///
/// `nixpkgs_from_rev` を catch-up 区間の起点（その nixpkgs rev を適用前状態とする）に使い、複数 bump を
/// 跨いだ適用をアプリ単位で集約した重要度連動表示にする（描画と集約は `update_history` の show 経路を
/// 再利用）。起点は dotfiles repo pin ではなく **nixpkgs rev** である（要約選択は `nixpkgs_old` と突合する
/// ため）。`stdout_is_terminal` が真なら起動元端末へ直接出力、偽（background daemon）なら `pending-summary` へ
/// **追記**して次回シェルで 1 回だけ消費させる（rev 単位の未表示分を失わないため上書きしない）。要約は
/// `last-run.log` へも残す。`--dry-run` では `pending-summary`/`last-run.log` へ書かず、tty 経路は stdout
/// 表示のみ行う。
///
/// tty 判定は呼び出し元（`run`）が `std::io::stdout().is_terminal()` を 1 回解決して `stdout_is_terminal` で
/// 注入する。本関数内では渡された bool で分岐し、内部から `is_terminal()` を呼ばない。これにより分岐が
/// アンビエント大域から切れ、テストは `stdout_is_terminal` を明示指定して tty/非 tty いずれの経路も決定論的に
/// exercise できる（stdout が tty になる nix build sandbox でも非 tty 経路を確実に検証できる）。
fn present_summary(
    state_dir: &Path,
    nixpkgs_from_rev: Option<&str>,
    dry_run: bool,
    stdout_is_terminal: bool,
) -> Result<()> {
    // 履歴は state dir のローカル複製（`<state-dir>/history`）から読む。`~/.config/dotfiles` には更新履歴が
    // 無く、適用時に input source から複製済みのこの dir を offline・決定論で参照する。
    let source = state_dir.join(HISTORY_LOCAL_SUBDIR);

    if stdout_is_terminal {
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

/// `pending-summary` へ適用要約ブロックを追記する（上書きしない）。完成済みブロックだけを公開する（C）。
///
/// 非 tty 適用ごとに 1 ブロックを末尾へ足す。daemon が連続適用しても未表示 rev を失わないよう追記で運用し、
/// 消費（表示と削除）は zsh フック（`config/zsh/auto-update.zsh`）が原子的 rename で 1 回だけ行うファイル
/// 契約とする。
///
/// **atomicity**: 旧実装は `pending-summary` を append open してから `render_applied_summary` が履歴を読んで
/// 書き込んだため、render 途中で失敗すると **部分的なブロックが `pending-summary` に残り**、その隙に消費側
/// （zsh フックの原子的 rename）が半端な内容を rename・表示・退避してしまう余地があった。これを避けるため、
/// まず同一 dir 内の temp ファイルへブロックを **完成させてから** 1 回の `write_all` で `pending-summary` 末尾へ
/// 追記する。render 失敗時は temp に閉じ、`pending-summary` には 1 バイトも触れない（部分内容を公開・消費させ
/// ない）。temp は同一 dir 内（rename と読取りが原子的なファイルシステム前提）に置き、追記後に削除する。
/// 完成ブロックの追記は 1 回の write でまとめて行うため、render の途中失敗による partial block は公開されない。
fn append_pending_summary(
    state_dir: &Path,
    source: &Path,
    nixpkgs_from_rev: Option<&str>,
) -> Result<()> {
    let path = state_dir.join(PENDING_SUMMARY);
    // まず temp ファイルへブロックを完成させる（render 途中失敗は temp に閉じ、pending-summary へ波及させない）。
    let temp_path = path.with_file_name(format!(
        "{PENDING_SUMMARY}.render.{}.tmp",
        std::process::id()
    ));
    let rendered = (|| -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        update_history::render_applied_summary(source, nixpkgs_from_rev, &mut buffer)?;
        Ok(buffer)
    })();
    let rendered = match rendered {
        Ok(buffer) => buffer,
        Err(error) => {
            // 念のため temp を残さない（render 自体は buffer 上で行うため通常 temp は未生成だが、保険で掃除）。
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
    };
    // 完成済みバイト列を temp へ書き、そこから 1 回の write で pending-summary 末尾へ追記する。
    fs::write(&temp_path, &rendered)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    let append_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        file.write_all(&rendered)
            .with_context(|| format!("failed to append to {}", path.display()))?;
        Ok(())
    })();
    let _ = fs::remove_file(&temp_path);
    append_result
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

/// 既存ロックを stale（孤児）とみなす経過時間（秒）。これより古い lock は奪取する。
///
/// プロセス kill / 再起動で `Drop` が走らないと lock ファイルが残り、`AlreadyExists` が永久 skip を招く。
/// 正常な適用（`nix flake update` + switch + darwin-rebuild）は分単位で完了しうるため、誤奪取（実行中の
/// 適用を別プロセスが横取り）を避けつつ孤児を確実に回収できる十分長い閾値を採る。6 時間は 1 日 1 回の
/// 適用サイクルより十分短く、かつ最長クラスの実適用より十分長い。
const LOCK_STALE_SECS: u64 = 6 * 60 * 60;

/// steal marker（`update.lock.steal`）を孤児とみなす経過時間（秒）。これより古い marker は回収する。
///
/// 奪取権を直列化する `update.lock.steal` は、奪取区間（古い lock の remove → 新 lock の create_new）の
/// **間だけ**存在する短命 marker である。区間中にプロセスが kill/OOM/電源断/abort されると `remove_file`
/// が走らず marker が恒久残骸化し、以後 `steal_stale_lock` が必ず `AlreadyExists` で `None`（skip）へ倒れ、
/// **stale lock の奪取が永久に起きなくなる**（marker 残骸が「stale lock を永久 skip しない」という機構自体の
/// 目的を破る）。これを防ぐため marker 自身に短い TTL を与え、TTL より古い marker は孤児とみなして回収し
/// 奪取権を再取得する。奪取区間（remove + create_new、I/O 数回）は秒オーダーで完了するため、誤回収（実行中の
/// 別プロセスの奪取権を横取り）を避けつつ孤児を速やかに掃除できる短さにする。5 分は実奪取区間より十分長く、
/// `LOCK_STALE_SECS` より十分短い。
const STEAL_MARKER_STALE_SECS: u64 = 5 * 60;

/// `update.lock` の `O_EXCL` ベース排他ロック。drop でロックファイルを除去する。
///
/// flock(2) を使うと `libc` 直呼び（禁止）か新規 crate が要るため、移植性とテスト容易性を優先し
/// `create_new`（`O_CREAT|O_EXCL`）でロックファイルを作る方式を採る。作成成功＝ロック取得、`AlreadyExists`＝
/// 取得失敗だが、**stale lock（プロセス kill/再起動で `Drop` 未実行のまま残った孤児）を永久 skip しない**よう、
/// 既存 lock の timestamp を見て一定時間（[`LOCK_STALE_SECS`]）より古ければ奪取する。lock ファイルはユーザ所有
/// state dir 配下に `pid\nepoch_secs` で書き、drop で除去する。`--dry-run` では実ロックファイルを作らず
/// （副作用なし）、常に取得成功として判定経路を通す。
struct UpdateLock {
    /// 取得したロックファイルのパス（drop で除去する）。`None` は dry-run（実ファイル無し）。
    path: Option<PathBuf>,
}

impl UpdateLock {
    /// ロックを非ブロッキングで試行する。取得成功で `Some`、生存中の既存ロックで `None` を返す。
    ///
    /// `AlreadyExists` 時は既存 lock の timestamp を見て staleness を判定する。stale（孤児）なら steal marker の
    /// `O_EXCL` 作成を CAS にして奪取権を 1 プロセスへ直列化し（[`steal_stale_lock`]）、勝者だけが新 lock を張る。
    ///
    /// 旧実装の「stale 判定 → `remove_file` → `create_new`」は、複数プロセスが同時に stale 判定した場合に、
    /// プロセス A が新 lock を張った直後にプロセス B の `remove_file` が **A の新 lock を消し**、双方が `create_new`
    /// に成功して二重奪取・二重適用へ至る race があった。奪取権を専用 marker の `O_EXCL` 作成で直列化することで、
    /// remove → `create_new` の区間が単一プロセスに閉じ、remove-clobber も live-lock 横取りも起きない。生存中
    /// （timestamp が新しい）なら奪取せず `None`（skip）を返す。`libc` を直呼びせず std のみで実現する。
    fn try_acquire(state_dir: &Path, dry_run: bool) -> Result<Option<Self>> {
        if dry_run {
            return Ok(Some(Self { path: None }));
        }
        let path = state_dir.join(LOCK_FILE);
        match Self::create_new_lock(&path)? {
            Some(lock) => Ok(Some(lock)),
            None => {
                // 既存 lock あり。stale（孤児）なら rename CAS で奪取、生存中なら skip。
                if Self::existing_lock_is_stale(&path) {
                    Self::steal_stale_lock(&path)
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// stale な lock を、専用 steal marker の `O_EXCL` 作成を CAS にして奪取する。勝者だけが新 lock を張る。
    ///
    /// **race-free CAS の要点**: 奪取の本体（古い lock の除去 → 新 lock 作成）を直接複数プロセスにやらせると、
    /// 「A が新 lock を張った直後に B の remove が A の新 lock を消す」「B が A の生存 lock を rename で横取りする」
    /// といった窓が生じ、二重奪取（同時に 2 つの排他が成立）に至る。これを根絶するため、**奪取権そのものを別
    /// ファイル（`update.lock.steal`）の `O_EXCL`（`create_new`）作成で 1 プロセスへ直列化**する。`create_new` は
    /// OS レベルで原子的に「ちょうど 1 人だけ成功」を保証するので、steal marker を作れた 1 プロセスだけが奪取
    /// 区間に入る。敗者は marker 作成に失敗して `None`（skip）へ倒れ、古い lock には一切触れない。
    ///
    /// 奪取区間に入った勝者は、**marker 取得後に改めて現在の lock を再判定**する（TOCTOU 回避）。marker 待ちの
    /// 間に別の勝者が既に lock を更新している可能性があるため、(1) lock が消えていれば `create_new` で張る、
    /// (2) まだ存在し stale なら remove → `create_new` で張替える、(3) 既に fresh（別勝者が更新済み）なら奪取を
    /// 諦める。これらは steal marker により単一プロセスへ直列化済みのため、remove と create_new の間に他者が
    /// 割り込むことはない。区間終了時に marker を必ず除去する。`libc` を直呼びせず std のみで実現する。
    fn steal_stale_lock(path: &Path) -> Result<Option<Self>> {
        let steal_marker = path.with_file_name(format!("{LOCK_FILE}.steal"));
        // 奪取権の CAS: steal marker を create_new できた 1 プロセスだけが奪取区間に入る。
        if !Self::claim_steal_marker(&steal_marker)? {
            // 別プロセスが奪取区間にいる（marker は新鮮）。古い lock に触れず skip する。
            return Ok(None);
        }
        // ここから先は steal marker により単一プロセスへ直列化された奪取区間。終了時に marker を必ず除去する。
        let outcome = Self::steal_within_marker(path);
        let _ = fs::remove_file(&steal_marker);
        outcome
    }

    /// steal marker（奪取権 CAS）を取得する。取得成功で `true`、別プロセスが新鮮に保持中なら `false`。
    ///
    /// 基本は `update.lock.steal` の `create_new`（`O_EXCL`）作成で「ちょうど 1 人だけ成功」を OS レベルに
    /// 直列化する。ただし marker 作成者が奪取区間中に kill/OOM/電源断/abort されると `remove_file` が走らず
    /// marker が恒久残骸化し、以後すべての奪取が `AlreadyExists` で永久 skip へ倒れて **stale lock を一切
    /// 奪取できなくなる**（fleet が静かに更新停止し自己回復しない）。これを防ぐため、`AlreadyExists` 時には
    /// 既存 marker の timestamp を見て [`STEAL_MARKER_STALE_SECS`] より古ければ **孤児とみなして回収**する:
    /// 古い marker を remove して `create_new` を 1 回だけ再試行し、勝てた 1 プロセスが奪取権を再取得する。
    /// marker が新鮮（実行中の別奪取者が保持）なら回収せず `false`（skip）へ倒し、横取りしない。回収の
    /// remove→create_new で別プロセスと競合しても、`create_new` の `O_EXCL` が同時成功を 1 人に絞るため
    /// 二重奪取は起きない（敗者は `AlreadyExists`→`false`）。読取り不能・timestamp 解析不能は保守的に
    /// 「新鮮」へ倒し（`is_stale_lock` の挙動）、誤回収を避ける。`libc` を直呼びせず std のみで実現する。
    fn claim_steal_marker(steal_marker: &Path) -> Result<bool> {
        match Self::create_new_marker(steal_marker)? {
            true => Ok(true),
            false => {
                // 既存 marker あり。孤児（TTL 超過）なら回収して再取得、新鮮なら奪取権を譲る。
                if Self::steal_marker_is_stale(steal_marker) {
                    let _ = fs::remove_file(steal_marker);
                    // 回収後の再取得。別プロセスが先に取り直していれば AlreadyExists→false で skip する。
                    Self::create_new_marker(steal_marker)
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// steal marker を `create_new`（`O_EXCL`）で作る。成功で `true`、既存（`AlreadyExists`）で `false`。
    ///
    /// 作成時は孤児回収（TTL 判定）用に `pid\nepoch_secs` を書く。timestamp 書込み失敗は致命にしない
    /// （その場合の staleness 判定は保守的に「新鮮」へ倒れ、誤回収を避ける）。
    fn create_new_marker(steal_marker: &Path) -> Result<bool> {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(steal_marker)
        {
            Ok(mut marker) => {
                let _ = write!(
                    marker,
                    "{}",
                    lock_payload(std::process::id(), now_epoch_secs())
                );
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(anyhow::Error::from(error).context(format!(
                "failed to create steal marker {}",
                steal_marker.display()
            ))),
        }
    }

    /// 既存 steal marker が孤児（TTL 超過）かを timestamp で判定する。
    ///
    /// marker 内容の epoch 秒が現在より [`STEAL_MARKER_STALE_SECS`] 以上古ければ孤児とみなす。読取り失敗・
    /// timestamp 解析不能・marker 消滅・未来時刻は保守的に「新鮮（孤児でない）」へ倒し、実行中の別奪取者の
    /// 奪取権を誤回収しない。
    fn steal_marker_is_stale(steal_marker: &Path) -> bool {
        let Ok(content) = fs::read_to_string(steal_marker) else {
            return false;
        };
        is_stale_lock(&content, now_epoch_secs(), STEAL_MARKER_STALE_SECS)
    }

    /// steal marker を保持した奪取区間内で、現在の lock 状態に応じて新 lock を張る（単一プロセス前提）。
    ///
    /// marker 取得待ちの間に別勝者が lock を更新した可能性があるため、marker 取得「後」に再判定する: lock 消滅
    /// なら新規作成、stale なら remove して張替え、fresh なら奪取せず `None`。marker により直列化済みのため、
    /// remove → `create_new` の間に他者が割り込まない（remove-clobber race が原理的に起きない）。
    fn steal_within_marker(path: &Path) -> Result<Option<Self>> {
        match Self::create_new_lock(path)? {
            // lock が消えていた（別勝者が解放済み）。そのまま新 lock を獲得。
            Some(lock) => Ok(Some(lock)),
            None => {
                // lock がまだ在る。stale なら remove して張替え、fresh（別勝者が更新済み）なら奪取しない。
                if Self::existing_lock_is_stale(path) {
                    let _ = fs::remove_file(path);
                    Self::create_new_lock(path)
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// `create_new`（`O_CREAT|O_EXCL`）で lock を新規作成する。成功で `Some`、既存（`AlreadyExists`）で `None`。
    ///
    /// 作成時は診断・staleness 判定用に `pid\nepoch_secs` を書く。timestamp 書込み失敗は致命にしない
    /// （その場合 staleness 判定は保守的に「生存中」へ倒れ、誤奪取を避ける）。
    fn create_new_lock(path: &Path) -> Result<Option<Self>> {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                let _ = write!(
                    file,
                    "{}",
                    lock_payload(std::process::id(), now_epoch_secs())
                );
                Ok(Some(Self {
                    path: Some(path.to_path_buf()),
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(anyhow::Error::from(error)
                .context(format!("failed to acquire lock {}", path.display()))),
        }
    }

    /// 既存 lock ファイルが stale（孤児）かを timestamp で判定する。
    ///
    /// lock 内容の epoch 秒が現在より [`LOCK_STALE_SECS`] 以上古ければ stale とみなす。読取り失敗・timestamp
    /// 解析不能・lock 消滅は保守的に「stale ではない（生存中）」へ倒し、実行中の適用を誤って横取りしない。
    fn existing_lock_is_stale(path: &Path) -> bool {
        let Ok(content) = fs::read_to_string(path) else {
            return false;
        };
        is_stale_lock(&content, now_epoch_secs(), LOCK_STALE_SECS)
    }
}

/// lock ファイルの内容（`pid\nepoch_secs`）を組み立てる純粋関数。
///
/// 1 行目は診断用 pid、2 行目は staleness 判定に使う取得時刻（UNIX epoch 秒）。
fn lock_payload(pid: u32, epoch_secs: u64) -> String {
    format!("{pid}\n{epoch_secs}\n")
}

/// lock 内容（`pid\nepoch_secs`）と現在時刻から staleness を判定する純粋関数。
///
/// 2 行目を epoch 秒として解析し、`now - acquired >= threshold` なら stale（孤児）とみなす。timestamp 行が
/// 無い / 解析不能 / 未来時刻（負の経過）は保守的に「stale ではない」へ倒し、生存中の適用を横取りしない。
/// 純粋関数として時刻・閾値を引数化し、奪取条件を I/O 無しで単体検証できるようにする。
fn is_stale_lock(content: &str, now_secs: u64, threshold_secs: u64) -> bool {
    let Some(acquired) = content
        .lines()
        .nth(1)
        .and_then(|line| line.trim().parse::<u64>().ok())
    else {
        return false;
    };
    now_secs
        .checked_sub(acquired)
        .is_some_and(|elapsed| elapsed >= threshold_secs)
}

/// 現在時刻を UNIX epoch 秒で返す（取得不能時は 0）。
///
/// `std::time` のみを使う（時刻 crate を導入しない）。epoch より前という異常時は 0 へ倒し、staleness 判定で
/// 誤って「無限に古い」と扱われないようにする（`is_stale_lock` 側で now < acquired は非 stale へ倒れる）。
fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
        DEFERRED_NIXPKGS_REV, DEFERRED_REV, LAST_APPLIED_NIXPKGS_REV, LAST_RUN_LOG,
        LAST_SUMMARIZED_NIXPKGS_REV, LOCK_FILE, LOCK_STALE_SECS, PENDING_SUMMARY,
        STEAL_MARKER_STALE_SECS, UpdateLock, append_pending_summary, clear_deferred_markers,
        copy_history_dir, is_stale_lock, lock_payload, parse_input_source_path, parse_nixpkgs_rev,
        parse_repo_pin, present_summary, read_deferred_nixpkgs_rev, read_deferred_rev,
        read_last_applied_nixpkgs_rev, read_last_applied_rev, read_last_summarized_nixpkgs_rev,
        resolve_state_dir, should_switch, update_args, write_deferred_nixpkgs_rev,
        write_deferred_rev, write_last_applied_nixpkgs_rev, write_last_applied_rev,
        write_last_summarized_nixpkgs_rev,
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

    #[test]
    fn is_stale_lock_uses_timestamp_threshold() {
        // P2-3: lock 内容（`pid\nepoch_secs`）の 2 行目を見て staleness を判定する純粋規則を固定。
        let now = 1_000_000u64;
        // 閾値以上古い → stale。
        assert!(is_stale_lock(
            &lock_payload(42, now - LOCK_STALE_SECS),
            now,
            LOCK_STALE_SECS
        ));
        assert!(is_stale_lock(
            &lock_payload(42, now - LOCK_STALE_SECS - 1),
            now,
            LOCK_STALE_SECS
        ));
        // 閾値未満（取得直後・実行中）→ 非 stale（横取りしない）。
        assert!(!is_stale_lock(&lock_payload(42, now), now, LOCK_STALE_SECS));
        assert!(!is_stale_lock(
            &lock_payload(42, now - 1),
            now,
            LOCK_STALE_SECS
        ));
        // timestamp 行が無い / 解析不能 / 未来時刻は保守的に非 stale。
        assert!(!is_stale_lock("42\n", now, LOCK_STALE_SECS));
        assert!(!is_stale_lock("42\nnotnum\n", now, LOCK_STALE_SECS));
        assert!(!is_stale_lock(
            &lock_payload(42, now + 100),
            now,
            LOCK_STALE_SECS
        ));
    }

    #[test]
    fn try_acquire_steals_stale_lock_but_skips_live_lock() -> crate::Result<()> {
        // P2-3 退行固定: プロセス kill 等で Drop されず残った stale lock は奪取して実行に進む。
        // 生存中（新しい timestamp）の lock は奪取せず skip する。
        let dir = temp_dir("lock-stale");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let lock_path = dir.join(LOCK_FILE);

        // 古い timestamp の孤児 lock を手で置く（Drop されなかった残骸を模す）。
        let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 60);
        std::fs::write(&lock_path, lock_payload(99999, stale_epoch)).expect("write stale lock");
        // 奪取して取得成功する。
        let acquired = UpdateLock::try_acquire(&dir, false)?;
        assert!(acquired.is_some(), "stale lock must be stolen");
        // 奪取後の lock は現在時刻で書き直され、生存中扱いになる（別プロセスは skip）。
        assert!(UpdateLock::try_acquire(&dir, false)?.is_none());
        drop(acquired);

        // 解放後、新しい（生存中）lock を置くと奪取されない。
        let fresh_epoch = super::now_epoch_secs();
        std::fs::write(&lock_path, lock_payload(12345, fresh_epoch)).expect("write fresh lock");
        assert!(
            UpdateLock::try_acquire(&dir, false)?.is_none(),
            "live lock must not be stolen"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn last_applied_nixpkgs_rev_round_trips_as_secondary_span_fallback() -> crate::Result<()> {
        // P2-2 退行固定（更新）: `last-applied-nixpkgs-rev` は適用成功時に確定する state file で、要約 span 起点
        // 解決の **二次フォールバック**（最優先は `last-summarized-nixpkgs-rev`。show-once 退行修正で追加）。
        // ローカル lock が new pin へ bump 済みでも、commit 経路で書かれたこの値が起点候補として残る。ここでは
        // state file の round-trip（不在→書込み→読取り、`last-applied-rev` と独立、dry-run 非書込み）を固定する。
        // 起点解決の優先順位そのものは `summarized_marker_takes_precedence_over_applied_for_span_start` が固定する。
        let dir = temp_dir("applied-nixpkgs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");

        // 不在時は None（呼び出し側は lock 更新前の nixpkgs rev へフォールバックする）。
        assert_eq!(read_last_applied_nixpkgs_rev(&dir)?, None);

        // 適用成功時に確定した nixpkgs rev を書く。
        write_last_applied_nixpkgs_rev(&dir, "nixpkgs-applied-old", false)?;
        assert_eq!(
            read_last_applied_nixpkgs_rev(&dir)?,
            Some("nixpkgs-applied-old".to_string())
        );
        // dotfiles pin の `last-applied-rev` とは別ファイルで独立に持つ。
        write_last_applied_rev(&dir, "dotfiles-pin", false)?;
        assert_eq!(
            read_last_applied_nixpkgs_rev(&dir)?,
            Some("nixpkgs-applied-old".to_string())
        );
        assert_eq!(
            read_last_applied_rev(&dir)?,
            Some("dotfiles-pin".to_string())
        );
        assert!(dir.join(LAST_APPLIED_NIXPKGS_REV).exists());

        // dry-run は書かない（確定済み値を上書きしない）。
        write_last_applied_nixpkgs_rev(&dir, "should-not-write", true)?;
        assert_eq!(
            read_last_applied_nixpkgs_rev(&dir)?,
            Some("nixpkgs-applied-old".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// `run()` の要約 span 起点解決を、nix 不要で決定論的に再現する test-only ヘルパ。
    ///
    /// 本番 `run()` は marker を `last-summarized-nixpkgs-rev`（最優先）→ `last-applied-nixpkgs-rev` →
    /// lock 更新前 nixpkgs rev の順で解決する。nix（lock 更新）を伴わずに本番と同じ優先順位を固定するため、
    /// lock フォールバック値は `lock_fallback` 引数で注入する（本番の `read_nixpkgs_rev` 相当）。
    fn resolve_span_start(state_dir: &Path, lock_fallback: &str) -> crate::Result<String> {
        Ok(match read_last_summarized_nixpkgs_rev(state_dir)? {
            Some(rev) => rev,
            None => match read_last_applied_nixpkgs_rev(state_dir)? {
                Some(rev) => rev,
                None => lock_fallback.to_string(),
            },
        })
    }

    /// 非 tty 適用 1 回ぶん（要約 → 要約済み marker 確定）を、本番 `run()` の defer 経路と同じ順序で実行する。
    ///
    /// 要約「後」に `last-summarized-nixpkgs-rev` を `applied_new` へ進める点が要点（partial-failure で要約前に
    /// 失敗すると marker が進まないことを別テストで固定する）。
    fn apply_once_defer(
        state_dir: &Path,
        lock_fallback: &str,
        applied_new: &str,
    ) -> crate::Result<()> {
        let span_start = resolve_span_start(state_dir, lock_fallback)?;
        // 非 tty 経路（background daemon の defer 適用）を決定論的に exercise するため stdout_is_terminal=false
        // を明示注入する。これで stdout が tty になる nix build sandbox でも pending-summary 追記経路を確実に通す。
        present_summary(state_dir, Some(span_start.as_str()), false, false)?;
        write_last_summarized_nixpkgs_rev(state_dir, applied_new, false)?;
        Ok(())
    }

    #[test]
    fn same_day_defer_runs_append_update_block_only_once() -> crate::Result<()> {
        // 退行固定（A: show-once）: 同日に shell catch-up（defer）と daemon home（defer）が両方走る通常ケースで、
        // pending-summary に同一更新ブロック（N0->N1）が **1 回だけ** 入ることを決定論的に固定する。
        //
        // marker（`last-summarized-nixpkgs-rev`）を要約「後」に進めるため、2 回目の present_summary は起点 = N1
        // → `select_entries` が `nixpkgs_old == N1` を見つけられず空 span → 再追記しない。defer 経路では
        // `last-applied-nixpkgs-rev` が更新されないため、旧実装（applied を span 起点に使用）はここで二度追記した。
        let dir = temp_dir("same-day-defer");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        // 履歴 chain: N0->N1（実 packages 1 件）。lock fallback（初回起点）は N0。
        write_history(&dir, &[("N0", "N1")]);

        // 1 回目（shell catch-up, defer）: 起点 = N0（marker 無し→ lock fallback）→ N0->N1 を追記し marker=N1。
        apply_once_defer(&dir, "N0", "N1")?;
        // 2 回目（daemon home, defer, 同日）: 起点 = marker(N1)。defer なので applied は依然未更新（None）。
        apply_once_defer(&dir, "N0", "N1")?;

        let pending = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).expect("read pending");
        // 更新ブロック（宣言アプリ行 neovim-N0）は 1 回だけ現れる（二度見え＝退行を弾く）。
        let occurrences = pending.matches("neovim-N0").count();
        assert_eq!(
            occurrences, 1,
            "same-day defer runs must append the N0->N1 block exactly once: {pending}"
        );
        // marker は要約済み終端 N1 に確定している（defer でも書く）。
        assert_eq!(
            read_last_summarized_nixpkgs_rev(&dir)?,
            Some("N1".to_string())
        );
        // defer 経路では適用 marker（`last-applied-nixpkgs-rev`）は進めない（rev 確定は commit が担う）。
        assert_eq!(read_last_applied_nixpkgs_rev(&dir)?, None);

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn partial_failure_before_summary_preserves_span() -> crate::Result<()> {
        // 退行固定（B: partial-failure 堅牢性）: switch/darwin が **要約前** に失敗した再実行で、要約 span が
        // 消えないことを固定する。要約「後」に marker を進める設計のため、要約前に失敗すれば marker は前回
        // 要約済み rev のまま保たれ、再実行で未表示範囲を再び示せる。
        let dir = temp_dir("partial-failure");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        // 履歴 chain: N0->N1->N2。N0->N1 は前回適用で要約済み、N1->N2 が今回の未表示範囲。
        write_history(&dir, &[("N0", "N1"), ("N1", "N2")]);

        // 前回の成功適用で N0->N1 を要約済み（marker = N1）にしておく。
        apply_once_defer(&dir, "N0", "N1")?;
        let _ = std::fs::remove_file(dir.join(PENDING_SUMMARY)); // 前回ぶんは消費済みとみなす。

        // 今回: lock は N2 へ bump 済みだが switch が要約「前」に失敗 → present_summary も marker 書込みも
        // 走らない。よって marker は N1 のまま。span 起点は marker(N1) を読む（bump 済み lock=N2 ではない）。
        let span_start = resolve_span_start(&dir, "N2")?;
        assert_eq!(
            span_start, "N1",
            "span start must be preserved at last-summarized rev, not the bumped lock rev"
        );

        // 再実行（switch 成功）で N1->N2 を要約できる（未表示範囲が消えていない）。
        apply_once_defer(&dir, "N2", "N2")?;
        let pending = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).expect("read pending");
        assert!(
            pending.contains("neovim-N1"),
            "re-run must still summarize the unshown N1->N2 range: {pending}"
        );
        // 二重表示にならないよう、再表示は未表示範囲（N1->N2）のみ。既表示 N0->N1 は出ない。
        assert!(
            !pending.contains("neovim-N0"),
            "already-summarized N0->N1 must not reappear: {pending}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn summarized_marker_takes_precedence_over_applied_for_span_start() -> crate::Result<()> {
        // 起点解決の優先順位を固定する: `last-summarized-nixpkgs-rev`（最優先）→ `last-applied-nixpkgs-rev`
        // → lock fallback。summarized は「最後に見せ終えた rev」で、適用 marker より新しくなりうる（defer 連続）。
        let dir = temp_dir("span-precedence");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");

        // どちらも無ければ lock fallback。
        assert_eq!(resolve_span_start(&dir, "lock-rev")?, "lock-rev");
        // applied のみ → applied。
        write_last_applied_nixpkgs_rev(&dir, "applied-rev", false)?;
        assert_eq!(resolve_span_start(&dir, "lock-rev")?, "applied-rev");
        // summarized があれば最優先。
        write_last_summarized_nixpkgs_rev(&dir, "summarized-rev", false)?;
        assert_eq!(resolve_span_start(&dir, "lock-rev")?, "summarized-rev");
        // round-trip と dry-run 非書込も固定。
        assert_eq!(
            read_last_summarized_nixpkgs_rev(&dir)?,
            Some("summarized-rev".to_string())
        );
        assert!(dir.join(LAST_SUMMARIZED_NIXPKGS_REV).exists());
        write_last_summarized_nixpkgs_rev(&dir, "should-not-write", true)?;
        assert_eq!(
            read_last_summarized_nixpkgs_rev(&dir)?,
            Some("summarized-rev".to_string())
        );

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

        // 非 tty 経路を `stdout_is_terminal=false` で決定論的に exercise し、pending-summary へ追記させる。
        // is_terminal() を注入化したため、stdout が tty になる環境（nix build sandbox）でも本経路を確実に通す。
        present_summary(&dir, Some("nA"), false, false)?;
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
        present_summary(&dir2, Some("dotfilespin-not-a-nixpkgs-rev"), false, false)?;
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
        // dry-run 契約: 非 tty・tty いずれの経路でも `pending-summary` / `last-run.log` を書かない（副作用抑止）。
        // is_terminal() を注入化したため tty 性をテストが制御でき、`stdout_is_terminal` の両値で副作用無しを固定する。
        let dir = temp_dir("present-dry");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create state+config dir");
        write_history(&dir, &[("nA", "nB"), ("nB", "nC")]);

        // 非 tty 経路の dry-run: pending-summary / last-run.log を書かない。
        present_summary(&dir, Some("nA"), true, false)?;
        assert!(
            !dir.join(PENDING_SUMMARY).exists(),
            "dry-run (non-tty) must not write pending-summary"
        );
        assert!(
            !dir.join(LAST_RUN_LOG).exists(),
            "dry-run (non-tty) must not write last-run.log"
        );

        // tty 経路の dry-run: 端末描画のみで last-run.log も書かない（pending は tty 経路では元々書かない）。
        present_summary(&dir, Some("nA"), true, true)?;
        assert!(
            !dir.join(PENDING_SUMMARY).exists(),
            "dry-run (tty) must not write pending-summary"
        );
        assert!(
            !dir.join(LAST_RUN_LOG).exists(),
            "dry-run (tty) must not write last-run.log"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn present_summary_tty_path_writes_last_run_log_not_pending() -> crate::Result<()> {
        // tty 経路（`stdout_is_terminal=true`）の副作用契約を、注入した bool で決定論的に固定する。
        // is_terminal() を呼び出し元注入にしたため、stdout が tty/非 tty どちらの環境（cargo test の pipe や
        // nix build sandbox の builder tty）でも、テストは tty 経路を明示指定して exercise できる。
        // tty 経路は起動元端末へ直接描画し、`pending-summary` は書かず（次回シェル消費は不要）、`last-run.log`
        // へは要約を残す。stdout 描画自体はキャプチャせず、観測可能なファイル副作用（pending 不在・log 存在）で固定。
        let dir = temp_dir("present-tty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        write_history(&dir, &[("nA", "nB"), ("nB", "nC")]);

        present_summary(&dir, Some("nA"), false, true)?;

        // tty 経路は pending-summary を書かない（非 tty の background 消費契約専用のため）。
        assert!(
            !dir.join(PENDING_SUMMARY).exists(),
            "tty path must not write pending-summary"
        );
        // tty 経路でも last-run.log には要約を残す（直近 1 回の適用内容を後追いできる）。
        let log = std::fs::read_to_string(dir.join(LAST_RUN_LOG)).expect("read last-run.log");
        assert!(
            log.contains("neovim-nA"),
            "tty path must record summary into last-run.log: {log}"
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
    fn parse_input_source_path_resolves_github_input_store_path() {
        // N3 退行固定（本番不全の核）: 本番の既定 source は `github:wthrk/dotfiles`（github 型 input）であり、
        // `nix flake metadata --json` の locked ノードは `path` キーを持たない（owner/repo/rev/narHash のみ）。
        // 旧実装は metadata の `locks.nodes.dotfiles.locked.path` を読むため github 型では常に None へ縮退し、
        // 本番で履歴複製が無音 no-op していた。修正は `nix flake archive --json` の `inputs.dotfiles.path`
        // （realize 済み store path）を読むため、github 型 input でも store path を解決できる。
        //
        // この fixture は実 `nix flake archive github:wthrk/dotfiles --json` の出力形に基づく:
        // root flake の `inputs` map に各 input が `inputs`（推移依存）+ `path`（store path）を持つ。
        let archive = r#"{
          "inputs": {
            "dotfiles": {
              "inputs": {
                "darwin": { "inputs": {}, "path": "/nix/store/aaa-darwin-source" },
                "nixpkgs": { "inputs": {}, "path": "/nix/store/bbb-nixpkgs-source" }
              },
              "path": "/nix/store/ccc-dotfiles-source"
            }
          },
          "path": "/nix/store/zzz-config-flake"
        }"#;
        assert_eq!(
            parse_input_source_path(archive, "dotfiles").as_deref(),
            Some("/nix/store/ccc-dotfiles-source")
        );
    }

    #[test]
    fn parse_input_source_path_handles_path_type_input() {
        // path 型 input（ローカル checkout を `path:` で指す等）でも、archive は同じ `inputs.<input>.path`
        // 形で realize 済み store path を返すため、github 型・path 型の両方で同一抽出経路が動くことを固定する。
        let archive = r#"{
          "inputs": {
            "dotfiles": { "inputs": {}, "path": "/nix/store/ddd-local-source" }
          },
          "path": "/nix/store/zzz-config-flake"
        }"#;
        assert_eq!(
            parse_input_source_path(archive, "dotfiles").as_deref(),
            Some("/nix/store/ddd-local-source")
        );
    }

    #[test]
    fn parse_input_source_path_falls_back_to_none_on_missing_or_broken() {
        // input が無い / path キーが無い形式は None へ縮退する（既存複製温存に倒す）。
        let no_input = r#"{ "inputs": {}, "path": "/nix/store/zzz-config-flake" }"#;
        assert!(parse_input_source_path(no_input, "dotfiles").is_none());
        let no_path = r#"{ "inputs": { "dotfiles": { "inputs": {} } } }"#;
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

    /// 非 tty 適用 1 回ぶんを、履歴複製の成否（`history_synced`）で要約・marker 確定を gate して実行する。
    ///
    /// 本番 `run()` は `sync_history` 成功時だけ present_summary → `last-summarized-nixpkgs-rev` 確定へ進む（A）。
    /// 失敗時は要約も marker 確定もせず span 起点を保つ。nix を伴わずにこの gate 挙動を固定するため、
    /// `history_synced` を引数で注入して run() の該当分岐と同じ順序を再現する。
    fn apply_once_defer_gated(
        state_dir: &Path,
        lock_fallback: &str,
        applied_new: &str,
        history_synced: bool,
    ) -> crate::Result<()> {
        if history_synced {
            let span_start = resolve_span_start(state_dir, lock_fallback)?;
            present_summary(state_dir, Some(span_start.as_str()), false, false)?;
            write_last_summarized_nixpkgs_rev(state_dir, applied_new, false)?;
        }
        Ok(())
    }

    #[test]
    fn sync_history_failure_does_not_advance_summarized_marker() -> crate::Result<()> {
        // A 退行固定: sync_history が失敗（履歴複製が無い）した適用では、要約も `last-summarized-nixpkgs-rev` の
        // 確定もしない。これにより、その rev の要約が永久に失われる（marker だけ進んで次回再表示されない）退行を
        // 防ぐ。次回 sync 成功時の再実行で未表示範囲を再び要約できる。
        let dir = temp_dir("sync-fail-marker");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        // 履歴 chain: N0->N1。span 起点（lock fallback）は N0。
        write_history(&dir, &[("N0", "N1")]);

        // 履歴複製失敗（history_synced=false）の適用: 要約も marker 確定もしない。
        apply_once_defer_gated(&dir, "N0", "N1", false)?;
        assert!(
            !dir.join(PENDING_SUMMARY).exists(),
            "sync 失敗時は要約（pending-summary）を書かない"
        );
        assert_eq!(
            read_last_summarized_nixpkgs_rev(&dir)?,
            None,
            "sync 失敗時は要約済み marker を進めない（その rev の要約を失わない）"
        );

        // 次回（履歴複製成功）で同じ範囲 N0->N1 を要約でき、marker が N1 へ進む（未表示範囲を取り戻す）。
        apply_once_defer_gated(&dir, "N0", "N1", true)?;
        let pending = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).expect("read pending");
        assert!(
            pending.contains("neovim-N0"),
            "再実行で未表示範囲 N0->N1 を要約する: {pending}"
        );
        assert_eq!(
            read_last_summarized_nixpkgs_rev(&dir)?,
            Some("N1".to_string()),
            "sync 成功後に marker が確定する"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn deferred_rev_round_trips_and_commit_uses_deferred_pin() -> crate::Result<()> {
        // B 退行固定: defer 時に控えた pin/nixpkgs rev を commit が確定する（commit 時に現在 pin を読み直さない）。
        // run() の home/darwin 二段は実適用を要するため、ここでは commit が参照する defer marker の I/O 契約
        // （round-trip・read_deferred 優先・dotfiles pin / nixpkgs の独立保持・dry-run 非書込）を直接固定する。
        let dir = temp_dir("deferred-rev");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");

        // defer 前は marker 不在（commit は現在 pin へフォールバックする）。
        assert_eq!(read_deferred_rev(&dir)?, None);
        assert_eq!(read_deferred_nixpkgs_rev(&dir)?, None);

        // defer 時点で「適用した pin」を控える。
        write_deferred_rev(&dir, "pin-applied-at-defer", false)?;
        write_deferred_nixpkgs_rev(&dir, "nixpkgs-applied-at-defer", false)?;
        assert_eq!(
            read_deferred_rev(&dir)?,
            Some("pin-applied-at-defer".to_string())
        );
        assert_eq!(
            read_deferred_nixpkgs_rev(&dir)?,
            Some("nixpkgs-applied-at-defer".to_string())
        );
        // 別ファイルで独立保持されている。
        assert!(dir.join(DEFERRED_REV).exists());
        assert!(dir.join(DEFERRED_NIXPKGS_REV).exists());

        // commit はこの defer 値を確定する（現在 pin を読み直さない）ことを、commit_rev_marker が読む経路と
        // 同じ read_deferred_rev で固定する。run() の commit 分岐は `read_deferred_rev(..).or_else(現在pin)` で
        // あり、defer 値があればそれを優先する。ここでは defer 値が確定対象になることを read で示す。
        assert_eq!(
            read_deferred_rev(&dir)?.as_deref(),
            Some("pin-applied-at-defer"),
            "commit は home 適用時点の defer pin を確定する（commit 時点の現在 pin ではない）"
        );

        // dry-run は控えを書かない（既存 defer 値を壊さない）。
        write_deferred_rev(&dir, "should-not-write", true)?;
        assert_eq!(
            read_deferred_rev(&dir)?,
            Some("pin-applied-at-defer".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn parse_repo_pin_falls_back_to_narhash_for_path_source_without_rev() -> crate::Result<()> {
        // F 退行固定: path source（`dotfiles init --source path:/...`）の dotfiles input は `locked.rev` を
        // 持たない。rev 必須にすると update 経路が path source で壊れるため、rev が無ければ narHash を pin 同一性へ
        // フォールバックする。github source（rev あり）は従来どおり rev で dedup する。
        let github_lock = r#"{
          "nodes": {
            "dotfiles": { "locked": { "rev": "ghrev123", "narHash": "sha256-gh", "type": "github" } }
          },
          "root": "root", "version": 7
        }"#;
        // github source は rev を pin にする（narHash があっても rev 優先）。
        assert_eq!(parse_repo_pin(github_lock, "dotfiles")?, "ghrev123");

        // path source は rev が無い。narHash を pin 同一性に使う（update が壊れない）。
        let path_lock = r#"{
          "nodes": {
            "dotfiles": {
              "locked": { "narHash": "sha256-pathcontent", "path": "/nix/store/aaa", "type": "path", "lastModified": 123 }
            }
          },
          "root": "root", "version": 7
        }"#;
        assert_eq!(parse_repo_pin(path_lock, "dotfiles")?, "sha256-pathcontent");

        // narHash も無ければ lastModified（数値）へ縮退する。
        let last_modified_only = r#"{
          "nodes": { "dotfiles": { "locked": { "path": "/nix/store/bbb", "type": "path", "lastModified": 456 } } },
          "root": "root", "version": 7
        }"#;
        assert_eq!(parse_repo_pin(last_modified_only, "dotfiles")?, "456");

        // rev/narHash/lastModified が一切無ければ pin 同一性を解決できず失敗（dedup 誤判定を避ける）。
        let no_identity = r#"{ "nodes": { "dotfiles": { "locked": { "type": "path" } } } }"#;
        assert!(parse_repo_pin(no_identity, "dotfiles").is_err());
        Ok(())
    }

    #[test]
    fn should_switch_works_for_path_source_narhash_pin() {
        // F: path source の pin（narHash）でも should_switch の dedup が成立する。内容が変われば narHash が
        // 動いて switch、同一なら skip する（rev source と同じ dedup 規則が narHash でも働く）。
        assert!(
            should_switch(Some("sha256-old"), "sha256-new"),
            "path source 内容変化（narHash 変化）は switch する"
        );
        assert!(
            !should_switch(Some("sha256-same"), "sha256-same"),
            "path source 内容不変（narHash 同一）は skip する"
        );
    }

    #[test]
    fn append_pending_summary_does_not_publish_partial_block_on_render_failure() -> crate::Result<()>
    {
        // C 退行固定: render 途中失敗で部分的な pending-summary を公開・消費させない。完成済みブロックだけを
        // temp 経由で 1 回 write する設計のため、履歴 source が壊れて render に失敗しても pending-summary は
        // 作られない（既存内容も汚さない）。ここでは履歴複製を欠いた state dir（render が空/失敗しうる source）で
        // 既存内容が温存され、temp ファイルが残骸として残らないことを固定する。
        let dir = temp_dir("pending-atomic");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");

        // 既存 pending-summary に確定済みブロックがある状態を作る。
        write_history(&dir, &[("r1", "r2")]);
        let source = dir.join(super::HISTORY_LOCAL_SUBDIR);
        append_pending_summary(&dir, &source, Some("r1"))?;
        let baseline = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).expect("read baseline");
        assert!(baseline.contains("neovim-r1"), "baseline block present");

        // render 用 temp が残骸として残っていないこと（成功時も掃除する契約）。
        let temp_leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("read state dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{PENDING_SUMMARY}.render."))
            })
            .collect();
        assert!(
            temp_leftovers.is_empty(),
            "render temp must not linger after append"
        );

        // 存在しない source（render が空ブロックになる）でも、既存の確定済みブロックは温存される
        // （部分公開・既存破壊が起きない）。
        let missing_source = dir.join("does-not-exist");
        append_pending_summary(&dir, &missing_source, Some("r1"))?;
        let after = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).expect("read after");
        assert!(
            after.contains("neovim-r1"),
            "existing committed block must be preserved: {after}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn concurrent_stale_steal_yields_single_winner_via_rename_cas() -> crate::Result<()> {
        // D 退行固定: stale lock を複数プロセスが同時奪取しても、rename ベースの CAS で **同時に保持される lock は
        // 高々 1 つ**になる。旧実装（read→remove_file→create_new）は A の新 lock を B の remove が消し、双方が
        // create_new に成功して二重奪取・二重適用しうる race があった。ここでは複数スレッドで同時に try_acquire し、
        // 奪取できた lock を **解放せず保持し続けたまま** 全スレッドの完了を待つ。誰も解放しないので、同時保持数が
        // そのまま「同時に成立した排他の数」になる。これが 1 を超えれば二重奪取＝退行。逐次の acquire→release→
        // acquire（正当）を winner 数に数え込まないよう、scope 終了まで lock を Vec に退避して保持する。
        let dir = temp_dir("steal-cas");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let lock_path = dir.join(LOCK_FILE);

        let contenders = 16;
        for _ in 0..64 {
            // 各ラウンドで古い孤児 lock を置き、全スレッドに stale と判定させる。
            let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 120);
            std::fs::write(&lock_path, lock_payload(99999, stale_epoch)).expect("write stale lock");

            let barrier = std::sync::Arc::new(std::sync::Barrier::new(contenders));
            // 取得できた lock を解放せず保持し続けるための退避先（同時保持数を測る）。
            let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::<UpdateLock>::new()));
            std::thread::scope(|scope| {
                for _ in 0..contenders {
                    let dir = dir.clone();
                    let barrier = std::sync::Arc::clone(&barrier);
                    let held = std::sync::Arc::clone(&held);
                    scope.spawn(move || {
                        barrier.wait();
                        if let Ok(Some(lock)) = UpdateLock::try_acquire(&dir, false) {
                            // 解放せず保持する（scope 終了まで生かして同時保持数を観測する）。
                            if let Ok(mut guard) = held.lock() {
                                guard.push(lock);
                            }
                        }
                    });
                }
            });
            let mut held = held.lock().expect("held lock");
            let count = held.len();
            assert!(
                count <= 1,
                "stale lock steal must yield at most one concurrently-held lock, got {count}"
            );
            // 保持していた lock を解放してから次ラウンドへ（drop で lock ファイルを除去）。
            held.clear();
            drop(held);
            // steal marker が残骸として残らない（奪取区間終了時に必ず除去される）。
            assert!(
                !dir.join(format!("{LOCK_FILE}.steal")).exists(),
                "steal marker must be cleaned up after the steal section"
            );
            let _ = std::fs::remove_file(&lock_path);
        }

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn orphan_steal_marker_does_not_permanently_skip_stale_lock_steal() -> crate::Result<()> {
        // 2 退行固定: steal marker（`update.lock.steal`）の孤児残骸が stale lock の奪取を永久 skip しないこと。
        //
        // 奪取区間中にプロセスが kill/OOM/電源断/abort されると `remove_file` が走らず marker が恒久残骸化し、
        // 以後すべての try_acquire が marker の `AlreadyExists` で永久 skip へ倒れて stale lock を一切奪取できなく
        // なる（fleet が静かに更新停止）。marker 自身に TTL を与えたため、TTL 超過の孤児 marker は回収され、
        // stale lock の奪取が再開する。
        let dir = temp_dir("orphan-steal");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let lock_path = dir.join(LOCK_FILE);
        let steal_marker = dir.join(format!("{LOCK_FILE}.steal"));

        // stale な孤児 lock（Drop されず残った残骸）を置く。
        let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 120);
        std::fs::write(&lock_path, lock_payload(99999, stale_epoch)).expect("write stale lock");

        // 孤児 steal marker（TTL 超過）を置く。これが残ると旧実装は AlreadyExists で永久 skip した。
        let stale_marker_epoch =
            super::now_epoch_secs().saturating_sub(STEAL_MARKER_STALE_SECS + 60);
        std::fs::write(&steal_marker, lock_payload(88888, stale_marker_epoch))
            .expect("write orphan steal marker");

        // 孤児 marker を回収して stale lock を奪取し、取得成功する（永久 skip しない）。
        let acquired = UpdateLock::try_acquire(&dir, false)?;
        assert!(
            acquired.is_some(),
            "orphan steal marker must be reclaimed so the stale lock can still be stolen"
        );
        // 奪取区間終了で marker は除去されている（残骸が累積しない）。
        assert!(
            !steal_marker.exists(),
            "steal marker must be cleaned up after the steal section"
        );
        drop(acquired);

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn fresh_steal_marker_is_not_reclaimed() -> crate::Result<()> {
        // 2 補完: 新鮮な steal marker（実行中の別奪取者が保持）は回収せず奪取権を譲る（横取りしない）。
        // marker が TTL 未満なら別プロセスが奪取区間にいるとみなし、try_acquire は None（skip）へ倒れる。
        let dir = temp_dir("fresh-steal");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let lock_path = dir.join(LOCK_FILE);
        let steal_marker = dir.join(format!("{LOCK_FILE}.steal"));

        // stale lock + 新鮮な steal marker（別プロセスが今まさに奪取中）。
        let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 120);
        std::fs::write(&lock_path, lock_payload(99999, stale_epoch)).expect("write stale lock");
        std::fs::write(&steal_marker, lock_payload(77777, super::now_epoch_secs()))
            .expect("write fresh steal marker");

        // 新鮮 marker は回収しない → skip（None）。別奪取者の lock/marker を横取りしない。
        assert!(
            UpdateLock::try_acquire(&dir, false)?.is_none(),
            "fresh steal marker (live stealer) must not be reclaimed"
        );
        // 新鮮 marker は残したまま（所有者が除去する）。
        assert!(
            steal_marker.exists(),
            "fresh steal marker must be left intact for its owner"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn new_cycle_clears_stale_deferred_markers_so_commit_does_not_confirm_unapplied_pin()
    -> crate::Result<()> {
        // 3 退行固定: deferred marker のサイクルローカル化。darwin 失敗等で commit へ到達せず deferred marker が
        // 残骸化しても、新サイクル冒頭の `clear_deferred_markers` がそれを消すため、後続サイクルの commit が
        // **このサイクルで適用していない古い defer 値を `last-applied` へ誤確定しない**（commit は marker 不在で
        // 現在 pin への後方互換縮退に倒れる）。
        let dir = temp_dir("cycle-local-defer");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");

        // 前サイクルの残骸 deferred marker（commit へ到達せず残った未適用 pin の控え）を模す。
        write_deferred_rev(&dir, "stale-pin-from-aborted-cycle", false)?;
        write_deferred_nixpkgs_rev(&dir, "stale-nixpkgs-from-aborted-cycle", false)?;
        assert_eq!(
            read_deferred_rev(&dir)?.as_deref(),
            Some("stale-pin-from-aborted-cycle")
        );

        // 新サイクル冒頭のクリア（run() が defer 書込み前に必ず呼ぶ）。
        clear_deferred_markers(&dir, false);

        // 残骸は消えている。これ以降に defer を経ずに commit が走っても、read_deferred_rev は None を返し
        // 古い未適用 pin を確定しない（現在 pin への縮退に倒れる）。
        assert_eq!(
            read_deferred_rev(&dir)?,
            None,
            "stale deferred pin must be cleared at new cycle start"
        );
        assert_eq!(
            read_deferred_nixpkgs_rev(&dir)?,
            None,
            "stale deferred nixpkgs rev must be cleared at new cycle start"
        );
        assert!(!dir.join(DEFERRED_REV).exists());
        assert!(!dir.join(DEFERRED_NIXPKGS_REV).exists());

        // dry-run はクリアしない（状態を触らない契約）。
        write_deferred_rev(&dir, "dry-run-keeps-this", false)?;
        clear_deferred_markers(&dir, true);
        assert_eq!(
            read_deferred_rev(&dir)?.as_deref(),
            Some("dry-run-keeps-this"),
            "dry-run must not clear deferred markers"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn locked_pin_identity_prefers_rev_then_narhash_then_last_modified() {
        // F: pin 同一性解決の優先順位（rev → narHash → lastModified）を純粋関数として固定する。
        use serde_json::json;
        assert_eq!(
            super::locked_pin_identity(&json!({ "rev": "R", "narHash": "N", "lastModified": 1 }))
                .as_deref(),
            Some("R")
        );
        assert_eq!(
            super::locked_pin_identity(&json!({ "narHash": "N", "lastModified": 1 })).as_deref(),
            Some("N")
        );
        assert_eq!(
            super::locked_pin_identity(&json!({ "lastModified": 42 })).as_deref(),
            Some("42")
        );
        assert_eq!(super::locked_pin_identity(&json!({ "type": "path" })), None);
    }
}

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
/// home 部分適用（`dotfiles update home`・非 defer）が最後に適用した dotfiles repo pin を控える state file。
///
/// **退行 finding 3374863446 の是正**: `last-applied-rev`（全体スコープ）は全体適用（target=all・非 defer）
/// でのみ確定する（[`commit_apply_markers`] / [`SwitchOptions::is_full_apply`]）。これは部分 target で全体 pin を
/// 確定すると未適用の他 target（darwin）が永久 starve するためである。しかし `last-applied-rev` だけだと、zsh
/// login catch-up が呼ぶ home-only `update home`（darwin を適用しない非 defer 経路）は **どの applied marker も
/// 確定せず**、`should_switch` が翌日以降も `previous_rev != current_pin` を真と判定して **同一 pin を毎ログイン
/// 再 switch する無限ループ**になる。
///
/// これを避けるため、home スコープ専用の applied marker を分離する。home-only catch-up は適用後にこの marker へ
/// 適用 pin を確定し、次回 home-only の適用要否（[`should_switch_home`]）はこの home marker（または全体適用が
/// 確定した `last-applied-rev`）を読んで同一 pin を dedup する。home marker は home スコープしか確定しないため、
/// 全体適用が読む `last-applied-rev` を動かさず、darwin は引き続き適用要と判定される（starve しない）。
const LAST_APPLIED_HOME_REV: &str = "last-applied-home-rev";
/// 最後に**適用に成功した**状態の推移的 nixpkgs rev（catch-up 要約 span の真の起点）。
///
/// 要約 span の起点に「lock 更新前のローカル lock の nixpkgs rev」を使うと、partial-failure 再実行で誤る:
/// 前回実行が `nix flake update` まで進んで lock を新 pin へ bump した後に switch/darwin で失敗すると、
/// `last-applied-rev` は古いまま・ローカル lock は新 pin になる。次回実行で lock 更新「前」に nixpkgs rev を
/// 読んでも、その値は既に bump 済みの新 rev であり、要約の old が new pin と一致して差分が消える。これを避け、
/// **最後に適用成功した時点の nixpkgs rev** をこのファイルへ確定書込みし、要約 span の起点に使う（未適用範囲の
/// 実起点を指す）。`last-applied-rev` と同時に書き、未確定（defer）時は書かない。
const LAST_APPLIED_NIXPKGS_REV: &str = "last-applied-nixpkgs-rev";
/// 最後に**要約を表示/追記し終えた**履歴エントリの `at`（RFC3339。catch-up 要約 span の `at` カーソル起点）。
///
/// catch-up 要約 span の起点は「適用済み rev」ではなく「**最後に利用者へ見せ終えたエントリ**」でなければ
/// show-once が壊れる。さらに**起点を nixpkgs rev にすると brew-only 更新を再表示する**: brew tap だけが進み
/// `nixpkgs_old == nixpkgs_new`（= 同一 nixpkgs rev `N`）のエントリが複数できると、nixpkgs rev では `N -> N`
/// を越えて進めず、要約済み brew-only 更新を毎回再選択してしまう。これを避けるため、span 起点は nixpkgs rev
/// ではなく**履歴エントリの `at`**（記録のたび前進する一意値。brew-only 夜でも進む）を単調カーソルにする。
///
/// **要約を append/表示し終えた直後**に、要約した範囲の終端エントリの `at`（`render_applied_summary` の戻り値）を
/// この marker へ確定書込みし（defer 経路でも commit 経路でも書く）、次回 present_summary はこの `at` を起点
/// （`after_at`）に読む。2 回目は起点 = 終端 `at` → `select_entries_after` がそれより後を選び空 → 再追記しない
/// （A: 二重抑止／brew-only 再表示抑止）。partial-failure では switch 失敗時に要約自体が走らずこの marker も
/// 進まないため、前回要約済み `at` が保たれ、再実行で未表示範囲を失わない（B: partial-failure 堅牢性）。
/// 要約「後」に書く点が `last-applied-*` と異なる。marker が無い（初回）ときは `None`（全件）から始める。
const LAST_SUMMARIZED_AT: &str = "last-summarized-at";
/// home-only NixOnly 要約（zsh ログイン catch-up の `update home`・非 defer）専用の要約済み `at` カーソル。
///
/// **退行 finding（運用整合）の是正**: `last-summarized-at`（全体/`All` スコープ）を home-only の **NixOnly** 要約と
/// 共有すると、選択 span に cask（brew）エントリが含まれていても home-only 要約が `select_entries_after` の filter
/// 「前」の selected 終端 `at` までカーソルを前進させてしまう。daemon（darwin）端末では launchd daemon の三段適用
/// （home defer → darwin → commit `All`）と zsh ログイン catch-up（`update home`・非 defer・NixOnly）が**同一 state
/// dir・同一 `last-summarized-at` を共有**するため、後者が先に走って cursor を cask エントリ越しに進めると、daemon
/// commit step の `All` 要約が空 span（「0アプリ更新」）になり、**darwin が実適用した cask がどの pending-summary
/// にも出ない（cask 要約 starve＝未表示要約喪失）**。
///
/// これを避け、`last-applied-rev` / `last-applied-home-rev` と同じ scope 分離方針で **home-only NixOnly 要約は
/// 独自のカーソル（本 marker）**で進める。daemon の `All` スコープは `last-summarized-at` を読み書きし続けるため、
/// home-only NixOnly 要約は `All` スコープのカーソルを一切動かさず、commit step の `All` 要約が cask を必ず 1 回
/// 要約できる（要件1: starve しない）。home-only 専用端末（daemon 無し・cask 非適用）では NixOnly 要約だけが走り、
/// 本 marker が NixOnly filter 後ではなく filter 前の選択 span 終端へ進む点は**この scope では正しい**（nix 更新は
/// home が実適用済みで、cask は NixOnly が表示しないため誤って「適用済み」と見せない。同じ nix 更新を毎回再表示
/// しない show-once も、本 marker が span 終端へ進むことで維持される。要件2）。`All` スコープと scope 別に持つため、
/// 既存の show-once（scope 内 1 回表示）・複数端末単一消費・catch-up 集約と矛盾しない（要件3）。
const LAST_SUMMARIZED_HOME_AT: &str = "last-summarized-home-at";
const PENDING_SUMMARY: &str = "pending-summary";
const LAST_RUN_LOG: &str = "last-run.log";
const LOCK_FILE: &str = "update.lock";
/// 最後に**全体適用（`--full`）で適用した** `flake.lock` 全体の identity（内容ダイジェスト）を控える state file。
///
/// 既定（非 `--full`）の `update` は dotfiles input だけを更新するため、dotfiles repo pin（`last-applied-rev`）の
/// 比較で適用要否を正しく判定できる（推移的 nixpkgs は dotfiles の committed lock に従属し、pin が動けば一緒に
/// 動く）。しかし `--full` は **全入力を最新解決へ更新する**ため、dotfiles rev が不変でも nixpkgs / framework
/// など他 input だけが動く通常ケースがある。この場合 dotfiles pin は変わらないので pin だけで判定すると
/// `should_switch` が skip し、`flake.lock` は更新されたのに switch が走らず新しい入力が実環境へ適用されない
/// （finding 3368636842）。これを避けるため、`--full` 時は lock 全体の identity（本ダイジェスト）の変化でも
/// switch を要否判定する。本 marker は `--full` の全体適用成功時にだけ確定し、非 `--full` 経路では pin ベース
/// 判定を維持する（dotfiles input だけの更新は pin が代表する）。
const LAST_APPLIED_LOCK_ID: &str = "last-applied-lock-id";
/// home 部分適用（`dotfiles update home --full`・非 defer）が最後に適用した `flake.lock` 全体 identity を控える
/// **home スコープ専用**の state file。
///
/// **finding 3376248543 の是正**: `dotfiles update home --full` では home-only 分岐が `options.full` 判定より先に
/// 選ばれるため、旧実装は home/full marker の repo pin（`last-applied-home-rev`）だけで skip 判定し、更新後
/// `flake.lock` の全体 identity を見なかった。dotfiles pin が同じまま `--full` で nixpkgs 等の他 input だけが
/// 変わる通常ケースでは、lock は更新済みなのに pin 一致で skip し home-manager switch が走らず新入力が home 環境へ
/// 適用されない。これを避け、home-only でも `--full` 時は lock 全体 identity の変化でも switch を要否判定する
/// （[`should_switch_home_full`]）。
///
/// **lock-id marker を home スコープへ分離する理由**: 全体スコープの `last-applied-lock-id` を home-only `--full` が
/// 確定すると、後続の全体 `--full` 適用（target=all）の [`should_switch_full`] が lock-id 一致で skip し、未適用の
/// darwin/system が starve する。`last-applied-home-rev` と同じ scope 分離方針で home-only `--full` は本 home
/// スコープ marker だけを確定し、全体スコープの `last-applied-lock-id` を動かさない。これで全体 `--full` は
/// 引き続き他 input 変化を検知でき、home-only `--full` の同一 lock 再適用も dedup できる。
const LAST_APPLIED_HOME_LOCK_ID: &str = "last-applied-home-lock-id";
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
/// defer→commit を 1 つの darwin 実行サイクルへ固定する **サイクル token**（ラッパーが生成し home/commit へ渡す）。
///
/// daemon ラッパーは home の `--defer-rev-marker` 終了後に user 側 `update.lock` を解放してから、別プロセスで
/// root の `darwin-rebuild`（`dotfiles switch darwin`）→ `--commit-rev-marker` を実行する。この darwin 実行中に
/// 別の login catch-up が新しい update サイクルを始めると、`clear_deferred_markers` 後に `deferred-rev` を上書き
/// でき、commit が **root が適用した pin ではなく後続サイクルの pin** を `last-applied` へ確定し、以後の darwin
/// 適用を skip させ得る（finding 3368519975。`update.lock` は darwin 実行中は解放済みのため排他で防げない）。
///
/// これを防ぐため、ラッパーは defer 直前に 1 サイクル分の token を生成し、home defer ステップへ
/// `--rev-marker-token <TOKEN>` で渡す。CLI は defer 時に `deferred-rev`/`deferred-nixpkgs-rev` と **同じ瞬間に**
/// この token を本 marker へ書く。ラッパーは同じ token を commit ステップへも渡し、CLI は commit 時に
/// **`deferred-token` が渡された token と一致する時だけ** `deferred-rev` を確定する。後続サイクルが `deferred-rev`
/// を上書きすれば `deferred-token` もそのサイクルの別 token に変わるため、root のサイクルの commit は不一致を検知
/// して **未適用 pin を確定しない**（確定を skip し、次回サイクルで再適用して収束）。token 無し（defer を経ない
/// 直接 commit・旧ラッパー）の経路は従来どおり現在 pin への後方互換縮退に倒れる。
const DEFERRED_TOKEN: &str = "deferred-token";

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

/// 別の `dotfiles update` が `update.lock` を保持していて適用を skip したことを zsh catch-up へ伝える専用 exit code。
///
/// **finding 3376248532 の是正**: CLI は別プロセスが lock を保持しているだけでも（実際には何も適用していなくても）
/// 終了する。zsh の `_dotfiles_auto_update_run_catchup` が終了ステータスだけで「当日 catch-up 成功」を判定すると、
/// 複数ログインで後発シェルが lock 競合 skip して当日 marker を成功扱いで書いた後、先行 update が network 失敗で
/// 落ちると、同日の後続シェルが再試行せず追随できない。よって lock 競合 skip だけは **exit 0（実適用成功）でも
/// exit 1（異常失敗）でもない専用コード**で返し、zsh は「実適用も up-to-date も確認できた」ときだけ当日 marker を
/// 確定する。`75`（`EX_TEMPFAIL`。sysexits 慣用の一時失敗＝再試行可）を採り、汎用失敗（1）と衝突させない。
pub(crate) const LOCK_CONTENDED_EXIT_CODE: u8 = 75;

/// `dotfiles update` の実行結果。終了コード変換（[`crate::cli`]）と zsh catch-up の marker 確定可否を分けるための区別。
///
/// - `Completed`: 実際に適用したか、適用済み pin と同一で up-to-date を確認したか、`--commit-rev-marker` を処理した
///   （= この実行が catch-up の責務を果たした）。zsh は当日 marker を確定してよい。exit 0。
/// - `LockContended`: 別の `dotfiles update` が lock を保持していて何も判定/適用できなかった。catch-up は未達成で
///   あり、zsh は当日 marker を確定してはならない（同日後続シェルが再試行できるよう開けておく）。専用 exit code。
pub(crate) enum UpdateOutcome {
    /// 適用 / up-to-date 確認 / commit 処理を完了した（catch-up 責務を果たした）。exit 0。
    Completed,
    /// lock 競合で skip した（catch-up 未達成）。[`LOCK_CONTENDED_EXIT_CODE`] で終了する。
    LockContended,
}

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
pub(crate) fn run(options: UpdateOptions) -> Result<UpdateOutcome> {
    let config_dir = options.switch.config_dir()?;
    switch::ensure_config_exists(&config_dir)?;

    let state_dir = state_dir()?;
    let dry_run = options.switch.dry_run();
    if !dry_run {
        // 状態ファイルはユーザ所有の state dir 配下にしか作らない。root では呼ばれない前提（auto-update.nix）。
        fs::create_dir_all(&state_dir)
            .with_context(|| format!("failed to create state dir {}", state_dir.display()))?;
    }

    // lock 取得失敗 = 他プロセスが適用中。この実行は何も判定/適用できていない（catch-up 未達成）ので、専用 exit
    // code を返して zsh が当日 marker を確定しないようにする（finding 3376248532）。次回シェル/スケジュールで再判定。
    let Some(_lock) = UpdateLock::try_acquire(&state_dir, dry_run)? else {
        println!("別の dotfiles update が適用中のため skip します");
        return Ok(UpdateOutcome::LockContended);
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
        // **token 一致検証**（finding 3368519975）: ラッパーから渡された `--rev-marker-token` と、defer 時に
        // 控えた `deferred-token` が一致する時だけ deferred 値を確定する。darwin 実行中（user lock 解放後）に別
        // サイクルが `deferred-rev`/`deferred-token` を上書きすれば token が変わるため、root のサイクルの commit は
        // 不一致を検知して **適用していない後続サイクルの pin を確定しない**。token 一致しない（または token 無し
        // で defer 値も無い）場合は、適用済みと確定せず次回サイクルへ収束を委ねる。
        let stored_token = read_deferred_token(&state_dir)?;
        let deferred_rev = read_deferred_rev(&state_dir)?;
        let deferred_nixpkgs_rev = read_deferred_nixpkgs_rev(&state_dir)?;
        match resolve_committed_marker(
            options.rev_marker_token.as_deref(),
            stored_token.as_deref(),
            deferred_rev.as_deref(),
            deferred_nixpkgs_rev.as_deref(),
        ) {
            CommitDecision::Confirm { pin, nixpkgs_rev } => {
                // defer 時点の値（pin あり）か、後方互換の現在値縮退（pin None）で確定する pin/nixpkgs rev を決める。
                let committed_pin = match pin {
                    Some(rev) => rev.to_string(),
                    None => read_repo_pin(&config_dir)?,
                };
                let committed_nixpkgs_rev = match nixpkgs_rev {
                    Some(rev) => rev.to_string(),
                    None => read_nixpkgs_rev(&config_dir)?,
                };

                // **要約を marker 確定「前」に行う（finding 3376248504）**: daemon フル経路は home step
                // （`--defer-rev-marker`）では要約を委譲し、darwin で実適用された brew cask を含む適用済み範囲はこの
                // commit step（home+darwin 両方適用済み）で `All`（nix + cask）として 1 回だけ要約する。旧実装は
                // `last-applied-*` を **先に確定**してから要約を best-effort で呼んでいたため、履歴 TOML 破損や
                // pending-summary 書込み失敗で要約だけ失敗すると、`last-summarized-at` が古いままでも次回は同一 pin と
                // 判定され早期 return に入り、未表示の darwin/cask を含む適用済み範囲が二度と再要約されなかった。
                // よって **要約成功後にだけ rev marker を確定**し、要約失敗時は rev を確定せず deferred marker も残す。
                // すると次の defer→commit サイクル（または skip 経路の `All` 要約再試行）が同一 pin で再要約を試せる
                // （switch/darwin は冪等再実行され、要約 cursor `last-summarized-at` は失敗時に進まないため未表示 span を
                // 失わない）。darwin drift 懸念（rev 未確定で darwin 再適用）は冪等再適用で安全側に倒れ、非 defer 経路
                // （finding 3368519980）と同じ「実作業成功後に marker 確定」方針に揃う。
                //
                // **scope = `All`（全体スコープ）**: span 起点と書き戻す marker は `last-summarized-at`（`All` スコープ
                // カーソル）を使う。home-only NixOnly 要約が動かす `last-summarized-home-at` とは分離されているため、
                // zsh ログイン catch-up が先に走って home カーソルを進めていても、この `All` 要約の span 起点は影響を
                // 受けず、darwin 実適用 cask を含む適用済み範囲を必ず 1 回要約できる（cask starve を防ぐ。要件1）。
                // ローカル履歴複製は home step の `sync_history` で取り込み済みのため、ここでは複製せず読むだけ。
                let summary_result =
                    present_and_commit_summary(&state_dir, SummaryScope::All, dry_run);
                match commit_writeback_plan(summary_result.is_ok()) {
                    CommitWriteback::Persist => {
                        // 要約成功 → rev marker を確定し、defer marker を消す（次サイクルへ古い値を持ち越さない）。
                        write_last_applied_rev(&state_dir, &committed_pin, dry_run)?;
                        write_last_applied_nixpkgs_rev(
                            &state_dir,
                            &committed_nixpkgs_rev,
                            dry_run,
                        )?;
                        clear_deferred_markers(&state_dir, dry_run);
                        println!("適用済み rev を確定しました（rev {committed_pin}）");
                    }
                    CommitWriteback::Defer => {
                        // 要約失敗 → rev を確定せず deferred marker も残し、次サイクルで再要約を試せるようにする
                        // （未表示 span を失わない）。darwin は冪等再適用される。要約 Err の文脈を stderr へ出す。
                        if let Err(error) = &summary_result {
                            eprintln!(
                                "適用済み範囲の要約に失敗しました（rev 未確定・次サイクルで再要約を試行）: {error}"
                            );
                        }
                    }
                }
            }
            CommitDecision::Skip => {
                // token 不一致 = 別サイクルが deferred 値を上書きした（root のサイクルの pin ではない）。未適用
                // pin を確定せず skip し、次回サイクルで再適用して収束させる。残骸 marker は次サイクル冒頭の
                // `clear_deferred_markers` が掃除する（ここで消すと別サイクルの正当な defer 値を奪うため触らない）。
                println!(
                    "rev マーカー token が不一致のため確定を skip します（別サイクルが deferred 値を更新）"
                );
            }
        }
        // commit 処理を実行した（catch-up 責務を果たした）。
        return Ok(UpdateOutcome::Completed);
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

    // catch-up 要約 span の起点となる「最後に利用者へ見せ終えた履歴エントリの `at`」は、要約直前に
    // `present_and_commit_summary` が **scope のカーソル**（`All` は `last-summarized-at`、`HomeOnlyNix` は
    // `last-summarized-home-at`）から読む。起点に要約済み `at` を使う理由:
    //   (A) 同日二重追記の抑止: shell catch-up（defer）と daemon home（defer）が同日に両方走っても、先行
    //       実行が要約後にこの marker を終端 `at` へ進めるため、後続は起点 = 終端 `at` → `select_entries_after`
    //       がそれより後を選び空 → 再追記しない。
    //   (A') brew-only 再表示の抑止: nixpkgs rev を起点にすると `nixpkgs_old == nixpkgs_new`（同一 nixpkgs rev）
    //       の brew-only 更新を毎回再選択してしまう。`at` は記録のたび前進する一意値なので `N -> N` を越えて進む。
    //   (B) partial-failure 堅牢性: switch/darwin が要約「前」に失敗するとこの marker は進まないため、前回
    //       要約済み `at` が保たれ、再実行で未表示範囲（要約 span）を失わない。
    // span 起点の読みは scope と対で `present_and_commit_summary` 内に閉じ、read/write カーソルが scope 間で
    // 食い違わないようにする（home-only NixOnly 要約が `All` スコープのカーソルを進める退行を構造的に防ぐ）。

    // **先に** ローカル lock を最新 repo pin へ更新する（skip 判定はこの後）。lock 更新前のローカル pin は前回
    // 適用値のまま動かないため、更新前に判定すると定常状態で常に skip し fleet が追随しない。lock 更新は冪等で
    // 副作用が小さいので、skip ケースでも先に走らせて upstream の新 pin を発見させる。
    update_lock(&config_dir, options.full, dry_run)?;

    // lock 更新後の dotfiles pin を読む。これが今回の適用対象（upstream の最新 repo pin）。
    let current_pin = read_repo_pin(&config_dir)?;

    // `--full` の switch 要否は lock 全体 identity の変化でも判定する（finding 3368636842）。`--full` は全入力を
    // 最新解決するため、dotfiles pin 不変でも nixpkgs/framework だけが動くケースがあり、pin だけ見ると skip して
    // 新入力が適用されない。`--full` 時は lock 全体ダイジェストを読み、pin か lock のいずれかが前回適用値と
    // 異なれば switch する。非 `--full` 経路は dotfiles input だけの更新なので従来どおり pin で判定する。
    let previous_rev = read_last_applied_rev(&state_dir)?;
    let should_apply = if options.switch.is_home_only_apply() && !options.defer_rev_marker {
        // home 部分適用（zsh login catch-up の `update home`・非 defer）は **home スコープ marker** で適用要否を
        // 判定する（finding 3374863446 の退行是正）。全体スコープの `last-applied-rev` は home-only では確定
        // しないため、これだけ見ると同一 pin を毎ログイン再 switch する。home marker（前回 home-only 適用 pin）
        // または全体適用が確定した `last-applied-rev`（全体適用は home を含むため home 適用済みとみなす）の
        // いずれかが current_pin に一致すれば skip する。home marker は home スコープしか確定しないため、全体
        // 適用（target=all）の `should_switch` は引き続き未適用 pin を switch 要と判定でき、darwin は starve しない。
        let home_rev = read_last_applied_home_rev(&state_dir)?;
        if options.full {
            // `update home --full`（finding 3376248543）: home-only 分岐が `options.full` より先に選ばれるため、
            // home/full marker の pin だけで skip すると dotfiles pin 不変 + 他 input だけ変化のケースで lock 更新済み
            // でも switch が走らない。`--full` 時は home スコープの lock-id（`last-applied-home-lock-id`）と現在の
            // lock 全体 identity を比較し、pin か lock のいずれかが変化していれば switch する。lock-id は home スコープ
            // marker を読むため、全体 `--full` 適用の dedup を壊さず darwin を starve させない。
            let current_lock_id = read_lock_id(&config_dir)?;
            let previous_home_lock_id = read_last_applied_home_lock_id(&state_dir)?;
            should_switch_home_full(
                home_rev.as_deref(),
                previous_rev.as_deref(),
                &current_pin,
                previous_home_lock_id.as_deref(),
                &current_lock_id,
            )
        } else {
            should_switch_home(home_rev.as_deref(), previous_rev.as_deref(), &current_pin)
        }
    } else if options.full {
        let current_lock_id = read_lock_id(&config_dir)?;
        let previous_lock_id = read_last_applied_lock_id(&state_dir)?;
        should_switch_full(
            previous_rev.as_deref(),
            &current_pin,
            previous_lock_id.as_deref(),
            &current_lock_id,
        )
    } else {
        should_switch(previous_rev.as_deref(), &current_pin)
    };
    if !should_apply {
        // lock 更新後の pin（`--full` では lock 全体）が前回適用済みと同一。switch / record / marker を skip する
        // （lock 更新は実施済み）。
        println!("適用済み pin と同一のため switch は不要です（rev {current_pin}）");
        // up-to-date を確認できた（catch-up 責務を果たした）。lock 競合とは区別して Completed を返す。
        return Ok(UpdateOutcome::Completed);
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

    // 要約を表示/追記し、その後で marker（`last-applied-*` / 要約済み `at`）を確定する。順序が要点で、要約「前」に
    // marker を進めると partial-failure（switch 後・要約前に異常終了）で未表示範囲や apply-dedup を失う。逆に要約
    // 「後」に確定することで、同日に defer 経路が連続しても 2 回目は起点 = new rev → 空 span → 再追記しない。
    //
    // 履歴複製が失敗（`history_synced == false`）したときは要約を skip する（A）。複製が無いまま空/古い履歴で要約
    // すると、見せていない rev について marker だけ進み、その rev の要約が永久に失われる。複製が成功した時だけ
    // 要約し、失敗時は span 起点を保って次回再試行に委ねる（last-applied は適用が進んだため後段で確定する）。
    //
    // **daemon フル経路（home defer）では要約を commit step へ委ねる（finding 運用整合・3368653947 と両立）**:
    // daemon は ② `update home --defer-rev-marker`（home 適用）→ ③ `switch darwin`（cask 適用、update を経由
    // しないため要約しない）→ ④ `update home --commit-rev-marker`（rev 確定）の三段で、home+darwin の両方を
    // 適用する。この home step（`defer_rev_marker == true`）でここで NixOnly 要約して marker を進めると、選択
    // エントリの終端 `at` が進む（cursor は filter 前の selected 全体から取るため）一方、darwin で実適用される
    // brew cask は NixOnly 表示で除外されるため、その cask が pending-summary のどの経路にも出ず starve する
    // （適用済み cask を通知しない欠落）。よって home defer step では要約も marker 確定も行わず、darwin 適用が
    // 完了した後の commit step（`--commit-rev-marker`）が `All`（nix + cask）で 1 回だけ要約して marker を進める。
    // これにより darwin で実適用した cask を含む適用済み範囲が要約される。
    //
    // 一方、**home-only catch-up（zsh、`update home` だが defer でない）は darwin を適用しない**ため、従来どおり
    // `NixOnly` で要約し、未適用の cask を通知しない（thread 3368653947 の意図を保つ）。判別キーは
    // `defer_rev_marker`: daemon フル経路（home+darwin 両方適用）でのみ home step の要約を commit へ委ねる。
    if history_synced && !options.defer_rev_marker {
        // 適用後要約は実際に適用した target に対応する出所だけへ絞る（finding 3368653947）。home-only catch-up
        // （`update home`・非 defer）は home-manager の nix package だけを switch するため **`HomeOnlyNix` scope**
        // （`NixOnly` + `last-summarized-home-at` カーソル）で brew cask を除外し、未適用の cask を通知しない。
        // darwin / 全体適用は **`All` scope**（systemPackages + cask + `last-summarized-at` カーソル）。
        //
        // **scope を分離する理由（運用整合 finding 是正）**: home-only NixOnly 要約は `select_entries_after` の
        // filter「前」の選択 span 終端 `at` までカーソルを進めるため、`All` と共有カーソルだと daemon 端末で
        // home-only 要約が cask エントリ越しに `All` スコープのカーソルを進め、commit `All` 要約が空 span になって
        // 実適用 cask が starve する。scope 別カーソル（`SummaryScope`）にすることで、home-only NixOnly 要約は
        // `All` スコープのカーソル（`last-summarized-at`）を動かさず、commit `All` 要約が cask を 1 回要約できる
        // （要件1）。home-only 専用端末では NixOnly 要約だけが home カーソルを進め、nix の show-once を維持しつつ
        // cask を「適用済み」と誤表示しない（要件2）。
        let scope = if options.switch.is_home_only_apply() {
            SummaryScope::HomeOnlyNix
        } else {
            SummaryScope::All
        };
        present_and_commit_summary(&state_dir, scope, dry_run)?;
    }

    // **marker 確定を「実作業（履歴同期 + 要約）成功後」に行う（finding 3368519980 / 3376248509）**: 全体適用
    // （target=all・非 defer）/ home 部分適用（非 defer）の apply-dedup marker（`last-applied-rev` /
    // `last-applied-nixpkgs-rev` / `last-applied-lock-id` / `last-applied-home-rev` / `last-applied-home-lock-id`）は、
    // 履歴同期（`history_synced`）と要約（`present_and_commit_summary`）が成功した後にだけ確定する。
    //
    // 要約失敗時に確定しない理由（3368519980）: 要約より先に last-applied を確定すると、履歴 TOML 破損や
    // pending-summary 書込み失敗で要約だけが Err 終了した場合に、次回 `should_switch` が同一 pin と判定して早期
    // return し、`last-summarized-at` が古いままでも未表示 span が二度と要約されない（要約失敗は上の `?` で伝播し、
    // ここに到達しないため last-applied を書かない）。
    //
    // **履歴同期失敗時も確定しない理由（finding 3376248509）**: 旧実装は `history_synced == false`（一時的な
    // archive/network 失敗で要約を skip した場合）でも「適用自体は進んでいる」として last-applied を確定していた。
    // すると次回同一 pin では `should_apply=false` で `sync_history` より前に早期 return し、通信復旧後も同じ pin
    // では履歴複製・適用後要約を再試行できず、次の bump まで `update-history show` / pending summary が空/古いまま
    // になる。よって履歴同期未成功時は apply-dedup marker を確定せず、次回同一 pin で switch（冪等）→ 再同期 →
    // 再要約を試せるようにする。`commit_apply_markers` へ `history_synced` を渡し、非 defer の apply-dedup 確定を
    // この成否で条件化する。defer 経路の deferred marker は要約を commit step へ委譲するため履歴同期失敗でも控える
    // （commit step が要約成功後に確定し、失敗時は次サイクルで再試行する＝finding 3376248504）。
    commit_apply_markers(
        &state_dir,
        &options,
        &config_dir,
        &current_pin,
        &applied_nixpkgs_rev,
        history_synced,
        dry_run,
    )?;
    // 実際に適用した（catch-up 責務を果たした）。
    Ok(UpdateOutcome::Completed)
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

/// `--full` 適用の switch 要否を、dotfiles pin と `flake.lock` 全体 identity の **いずれかの変化**で決める純粋関数。
///
/// `--full` は全入力を最新解決へ更新するため、dotfiles rev が不変でも nixpkgs / framework など他 input だけが
/// 動く通常ケースがある。pin だけで判定する [`should_switch`] では、その場合 `flake.lock` は更新されたのに skip し、
/// 新しい入力が実環境へ適用されない（finding 3368636842）。本関数は (a) pin が前回適用値と異なる、または
/// (b) lock 全体 identity が前回 `--full` 適用値（`last-applied-lock-id`）と異なるとき `true`（switch）を返す。
/// `previous_lock_id` が `None`（`--full` 初回・本機能導入前）なら lock 未適用とみなして switch する。これにより
/// dotfiles rev 不変でも他 input が動けば確実に switch し、両方とも前回値と同一なら skip する。判定を I/O から
/// 切り離し、「pin 同一でも lock 変化なら switch / pin も lock も同一なら skip」を単体検証可能にする。
fn should_switch_full(
    previous_rev: Option<&str>,
    current_pin: &str,
    previous_lock_id: Option<&str>,
    current_lock_id: &str,
) -> bool {
    should_switch(previous_rev, current_pin) || previous_lock_id != Some(current_lock_id)
}

/// home 部分適用（`update home`・非 defer）の switch 要否を **home スコープ marker** で決める純粋関数。
///
/// home-only catch-up が同一 pin を毎ログイン再 switch する退行（finding 3374863446）を防ぐための判定。home
/// 部分適用は全体スコープの `last-applied-rev` を確定しない（部分 target で全体 pin を確定すると darwin が永久
/// starve するため）ので、home-only の dedup には home 専用 marker（`last-applied-home-rev`）が要る。
///
/// `home_rev` は前回 home-only 適用が確定した home スコープ pin、`full_rev` は全体適用が確定した
/// `last-applied-rev`（全体適用は home を含むため home 適用済みの根拠になる）。いずれかが `current_pin` に一致
/// すれば home は適用済みなので `false`（skip）、どちらも一致しなければ `true`（switch）を返す。両 marker とも
/// `None`（初回）なら一致せず必ず switch する。この関数は home スコープしか参照しないため、全体適用の
/// [`should_switch`] が darwin 側の適用要否を独立に判定でき、home-only 適用後も darwin は starve しない。
fn should_switch_home(home_rev: Option<&str>, full_rev: Option<&str>, current_pin: &str) -> bool {
    home_rev != Some(current_pin) && full_rev != Some(current_pin)
}

/// home 部分適用 + `--full`（`update home --full`・非 defer）の switch 要否を、home スコープ pin と
/// `flake.lock` 全体 identity の **いずれかの変化**で決める純粋関数（finding 3376248543）。
///
/// `update home --full` は home-only 分岐が `options.full` 判定より先に選ばれるため、pin だけ見る
/// [`should_switch_home`] では dotfiles pin 不変 + 他 input（nixpkgs 等）だけ変化のケースで lock 更新済みでも
/// skip し、home-manager switch が走らない。本関数は (a) home スコープ pin 判定（[`should_switch_home`]）が
/// switch 要、または (b) lock 全体 identity が **home スコープの前回 `--full` 適用値**（`last-applied-home-lock-id`）
/// と異なるとき `true`（switch）を返す。`previous_home_lock_id` が `None`（home `--full` 初回・本機能導入前）なら
/// lock 未適用とみなして switch する。
///
/// lock-id 比較は **home スコープ marker** を読む（全体 `last-applied-lock-id` ではない）。全体 marker を読むと
/// home-only と全体 `--full` がカーソルを共有して darwin starve / 誤 dedup を起こすため、`should_switch_home` が
/// home/全体 pin を scope 分離するのと同じ方針で lock-id も home スコープへ分離する。判定を I/O から切り離し、
/// 「pin 同一でも lock 変化なら switch / pin も lock も同一なら skip」を単体検証可能にする。
fn should_switch_home_full(
    home_rev: Option<&str>,
    full_rev: Option<&str>,
    current_pin: &str,
    previous_home_lock_id: Option<&str>,
    current_lock_id: &str,
) -> bool {
    should_switch_home(home_rev, full_rev, current_pin)
        || previous_home_lock_id != Some(current_lock_id)
}

/// `--commit-rev-marker` の token 検証結果。確定するか、別サイクル上書き検知で確定を skip するか。
///
/// `Confirm` の `pin`/`nixpkgs_rev` が `Some` なら defer 時点で控えた値を確定し、`None` なら後方互換で現在値へ
/// 縮退する（token も deferred 値も無い旧経路）。`Skip` は token 不一致（別サイクルが deferred 値を上書きした）で、
/// root が適用していない pin の確定を避けるために確定自体を見送る（finding 3368519975）。
enum CommitDecision<'a> {
    /// 確定する。`pin`/`nixpkgs_rev` が `Some` なら defer 値、`None` なら現在値縮退。
    Confirm {
        pin: Option<&'a str>,
        nixpkgs_rev: Option<&'a str>,
    },
    /// token 不一致のため確定を skip する（別サイクルが deferred 値を上書きした）。
    Skip,
}

/// commit が確定すべき pin/nixpkgs rev を、サイクル token の一致検証込みで決める純粋関数（finding 3368519975）。
///
/// 判定:
/// - `passed_token` と `stored_token` が **両方 `Some` で一致** → このサイクルの defer 値を確定する
///   （`Confirm { pin: deferred_rev, nixpkgs_rev: deferred_nixpkgs_rev }`）。darwin 実行中に別サイクルが
///   `deferred-rev`/`deferred-token` を上書きしていれば `stored_token` がそのサイクルの別 token になり一致しない。
/// - `passed_token` と `stored_token` が **両方 `Some` で不一致** → `Skip`。別サイクルが deferred 値を上書きした
///   ので、root のサイクルの commit は適用していない後続サイクルの pin を確定しない。
/// - `passed_token` が `None`（token 無しの後方互換経路・旧ラッパー・defer を経ない直接 commit）→ 従来挙動。
///   `deferred_rev` があればそれを確定（`Confirm { pin: deferred_rev, .. }`）、無ければ現在 pin へ縮退
///   （`Confirm { pin: None, .. }`）。token 検証は要求されていないため skip しない。
/// - `passed_token` が `Some` だが `stored_token` が `None`（token を渡したのに defer が token を控えていない =
///   別サイクルが defer 値を clear した／defer を経ていない）→ `Skip`。検証要求があるのに照合相手が無いので、
///   未適用 pin の誤確定を避けて確定しない。
///
/// 判定を I/O から切り離し、token 一致/不一致/無しの各分岐を単体検証できるようにする。
fn resolve_committed_marker<'a>(
    passed_token: Option<&str>,
    stored_token: Option<&'a str>,
    deferred_rev: Option<&'a str>,
    deferred_nixpkgs_rev: Option<&'a str>,
) -> CommitDecision<'a> {
    match passed_token {
        // token を渡された: 一致検証を要求する経路。
        Some(passed) => match stored_token {
            Some(stored) if stored == passed => CommitDecision::Confirm {
                pin: deferred_rev,
                nixpkgs_rev: deferred_nixpkgs_rev,
            },
            // 不一致、または照合相手の stored token が無い → 別サイクル上書き等。確定しない。
            _ => CommitDecision::Skip,
        },
        // token 無し（後方互換）: 従来どおり deferred 値 → 現在値縮退で確定する（token 検証はしない）。
        None => CommitDecision::Confirm {
            pin: deferred_rev,
            nixpkgs_rev: deferred_nixpkgs_rev,
        },
    }
}

/// commit（`--commit-rev-marker`）の `Confirm` 分岐で、`All` 要約の成否に応じた marker writeback の可否を表す。
///
/// `Persist` は rev marker（`last-applied-rev` / `last-applied-nixpkgs-rev`）を確定し deferred marker を clear する。
/// `Defer` は何も書き戻さず（rev 未確定・deferred marker 残置）、次サイクルでの再要約に委ねる。
#[derive(Debug, PartialEq, Eq)]
enum CommitWriteback {
    /// 要約成功 → rev marker を確定し deferred marker を clear する。
    Persist,
    /// 要約失敗 → rev を確定せず deferred marker も残す（再要約のため）。
    Defer,
}

/// commit `Confirm` 分岐の writeback gating を `All` 要約の成否だけから決める純粋関数（finding 3376248504）。
///
/// 要約成功時のみ `last-applied-rev` / `last-applied-nixpkgs-rev` を確定し deferred marker を clear（`Persist`）、
/// 要約失敗時は rev を確定せず deferred marker を残す（`Defer`）。これは「実作業（要約表示）成功後にだけ marker を
/// 確定する」契約であり、要約失敗で marker を先に進めて未表示 span を失う退行を防ぐ。判定を I/O から切り離し、
/// 要約成否を `bool` で注入して両分岐を決定論的に固定できるようにする。
fn commit_writeback_plan(summary_succeeded: bool) -> CommitWriteback {
    if summary_succeeded {
        CommitWriteback::Persist
    } else {
        CommitWriteback::Defer
    }
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
    sync_history_from_source(resolve_input_source(config_dir), state_dir)
}

/// 解決済み source（`Some` = archive 成功、`None` = archive 失敗）から履歴複製の成否を決める分離点。
///
/// **archive 失敗（`None`）は同期未成功として `Err` を返す**（finding 3368519977）。`resolve_input_source` は
/// network 無し・nix 不在・archive 失敗・JSON 解析失敗をすべて `None` に畳むため、ここで `Ok(())` にすると
/// 呼び出し側が `history_synced = true` とみなし、空/古い履歴で要約して `last-summarized-at` を進め、その span の
/// 要約が永久に失われる（後で履歴を複製できても表示済み扱いになる）。よって `None` は `Err` にして要約 marker の
/// 確定を抑止し、適用自体は呼び出し側が best-effort の警告に留めて続行する。`Some(source)` でも履歴 dir が
/// 実在しない場合は「複製対象が無い正常系」として `Ok(())`（archive 失敗と区別する。履歴が無いので marker を
/// 進めても失う要約が無く、present_summary が空 → marker 据置に倒れる）。source 解決を引数化し、archive 実行を
/// 伴わずに「archive 失敗 → Err / source あり履歴無し → Ok」の分岐を単体検証できるようにする。
fn sync_history_from_source(source_root: Option<PathBuf>, state_dir: &Path) -> Result<()> {
    let source_root = source_root.ok_or_else(|| {
        anyhow!(
            "failed to resolve dotfiles input source via `nix flake archive` (history not synced; \
             summary deferred to next run)"
        )
    })?;
    let source_history = source_root.join(HISTORY_SUBDIR);
    if !source_history.is_dir() {
        // source は解決できたが履歴 dir が無い（source に `docs/update-history` が無い正常系）。
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
/// **複製前に dest を空へ作り直す**（finding 3368677389）。単に上書きコピーするだけだと、source 側で月次 TOML が
/// 削除/リネームされた pin に更新しても、前の pin で複製済みの古い `*.toml` が dest に残り続け、`update-history
/// show` と適用後要約がその削除済み・改名済み履歴を読み続けて表示に混入する。これを避けるため、コピー前に dest を
/// **一時 dir へ新規構築 → 完成後に既存 dest と atomic に rename 置換**する。temp で全ファイルを作り終えてから
/// 1 回の rename で差し替えるので、読み手（show / 要約）は「古い完全な複製」か「新しい完全な複製」のどちらかだけを
/// 観測し、削除途中・コピー途中の中間状態（半端な TOML 集合）を見ない。temp は dest と同一親 dir 内に置き、rename
/// が同一ファイルシステム内で原子的に成立することを前提にする。source 直下の通常ファイルだけを名前ごとコピーする
/// （サブディレクトリは履歴 layout 上想定しないため対象外）。store path 由来 source は read-only なため、複製先は
/// ユーザ所有 state dir に置いて以降の読取りを保証する。コピー失敗は呼び出し側が best-effort として扱えるよう
/// `Err` を返す（致命にしないのは呼び出し側の責務）。失敗時は temp を掃除し、既存 dest は壊さない。
///
/// **置換失敗時に旧複製を喪失しない**（finding 3374863441）: 単に `remove_dir_all(dest)` を先に行ってから
/// `rename(temp, dest)` すると、その rename が失敗した場合（temp と dest が別ファイルシステムにある等）に旧複製が
/// 既に消えていて、`sync_history` が `Err` を返すと呼び出し側はそれを警告へ落として `last-applied-*` を確定するため、
/// 次回同じ pin では switch/sync が skip され `update-history show` や適用後要約が空/古い状態から自己回復しない。
/// よって dest を先に消さず、**既存 dest を backup へ rename 退避 → temp を dest へ rename → 成功時に backup を削除、
/// 失敗時は backup を dest へ rename 復元**の順にして、置換失敗時に旧複製を残す。
fn copy_history_dir(source_history: &Path, dest: &Path) -> Result<()> {
    // dest と同一親 dir 内の一時 dir に新しい複製を構築する（rename での atomic 置換を成立させるため）。
    let parent = dest.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create history parent {}", parent.display()))?;
    let dest_name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| HISTORY_LOCAL_SUBDIR.to_string());
    let temp_dir = parent.join(format!("{dest_name}.sync.{}.tmp", std::process::id()));
    // 前回の中断で残った temp があれば消してから作り直す（古い残骸を取り込まない）。
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

    // 完成した temp を、旧複製を喪失せずに dest へ原子的に差し替える。
    let backup_dir = parent.join(format!("{dest_name}.backup.{}.tmp", std::process::id()));
    replace_history_dir_atomically(&temp_dir, dest, &backup_dir)
}

/// 完成済み複製 `temp_dir` を、旧複製を喪失せずに `dest` へ差し替える（finding 3374863441）。
///
/// **置換失敗時に旧複製を残す退避/復元規律**: dest を先に `remove_dir_all` してから `rename(temp, dest)` すると、
/// その rename が失敗（temp と dest が別ファイルシステムにある等）した瞬間に旧複製を失う。呼び出し側はその失敗を
/// best-effort の警告へ落として `last-applied-*` を確定するため、次回同じ pin では switch/sync が skip され、
/// `update-history show` や適用後要約が空/古い状態から自己回復しない。よって順序を次のように固定する:
///
/// 1. 既存 dest を `backup_dir` へ rename 退避する（dest が無い初回 = `NotFound` は退避不要）。
/// 2. `temp_dir` を dest へ rename する。
/// 3. 成功時のみ `backup_dir` を削除する。
/// 4. 失敗時は temp を掃除し、退避した旧複製を `backup_dir` → dest へ rename 復元して喪失を防ぐ。
///
/// 退避 rename 自体が `NotFound` 以外で失敗した場合は dest を破壊していないので、そのまま置換へ進む（置換成功なら
/// 新複製で上書きされ、置換失敗でも dest は元位置に残る）。`backup_dir` は呼び出し側が dest と同一親 dir 内の一意名で
/// 与える前提。`libc` を直呼びせず std の rename / remove のみで実現する。
fn replace_history_dir_atomically(temp_dir: &Path, dest: &Path, backup_dir: &Path) -> Result<()> {
    let _ = fs::remove_dir_all(backup_dir);
    let backed_up = match fs::rename(dest, backup_dir) {
        Ok(()) => true,
        // dest が無い（初回複製）場合は退避するものが無い。それ以外の退避失敗も致命にせず、dest を破壊しないまま
        // 後続の置換へ進む（置換が成功すれば新複製で上書きされ、失敗時は dest がそのまま残る）。
        Err(_) => false,
    };
    if let Err(error) = fs::rename(temp_dir, dest) {
        // 置換に失敗した。temp を掃除し、退避した旧複製を dest へ戻して喪失を防ぐ。
        let _ = fs::remove_dir_all(temp_dir);
        if backed_up {
            let _ = fs::rename(backup_dir, dest);
        }
        return Err(anyhow::Error::from(error).context(format!(
            "failed to atomically replace history dir {}",
            dest.display()
        )));
    }
    // 置換成功。退避した旧複製はもう不要なので削除する。
    if backed_up {
        let _ = fs::remove_dir_all(backup_dir);
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

/// 生成ローカル flake の `flake.lock` 全体の identity（内容ダイジェスト）を読む。
///
/// `--full` 適用の要否判定で「dotfiles pin は不変だが nixpkgs/framework など他 input だけが動いた」ケースを
/// 検知するために使う（finding 3368636842）。`flake.lock` の **全バイト**から決定論的ダイジェストを計算し、
/// 16 進文字列で返す。lock の内容が 1 バイトでも変われば identity が動くため、どの input が動いても変化を捉える。
/// lock 不在・読取り失敗は文脈付き `Err`（識別子不定で誤って skip しないため）。
fn read_lock_id(config_dir: &Path) -> Result<String> {
    let lock_path = config_dir.join("flake.lock");
    let bytes =
        fs::read(&lock_path).with_context(|| format!("failed to read {}", lock_path.display()))?;
    Ok(lock_content_id(&bytes))
}

/// `flake.lock` の全バイトから決定論的な内容 identity（16 進文字列）を計算する純粋関数。
///
/// FNV-1a 64bit を std だけで実装する（外部 crate / 暗号 hash を導入しない。用途は「同一バイナリが自分の前回
/// 適用値と比較する変化検知」だけで、衝突耐性や cross-version 安定性は不要）。同じバイト列には常に同じ id を返し、
/// 1 バイトでも異なれば（高確率で）異なる id を返す。`std::hash::DefaultHasher` は安定性が保証されないため使わず、
/// アルゴリズムを固定して変化検知の決定性を保つ。抽出を I/O から切り離し、lock 内容変化の検知を単体検証できる。
fn lock_content_id(bytes: &[u8]) -> String {
    // FNV-1a 64bit。offset basis と prime は標準値。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
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

/// 最後に要約を表示/追記し終えた履歴エントリの `at`（RFC3339）を読む（不存在/空なら `None`）。
///
/// `None`（このコード導入前に適用済み・本当の初回）なら呼び出し側は `after_at = None`（全件 = 初回 catch-up）
/// から始める。`at` カーソルは brew-only `N -> N` 更新も越えて進むため、nixpkgs rev へのフォールバックは
/// 持たない（rev は `at` ではなく、`select_entries_after` の `after_at` に渡すと誤選択するため）。
fn read_last_summarized_at(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_SUMMARIZED_AT))
}

/// home-only NixOnly 要約専用の要約済み `at` カーソルを読む（不存在/空なら `None`）。
///
/// `All` スコープの `last-summarized-at` とは分離した home スコープ専用カーソル（運用整合 finding 是正）。
/// zsh ログイン catch-up（`update home`・非 defer・NixOnly）の span 起点はこの home カーソルを読み、daemon の
/// `All` スコープ（`last-summarized-at`）を動かさないことで commit step の cask 要約を starve させない。
/// `None`（初回・本 marker 導入前）なら NixOnly 要約は `after_at = None`（全件）から始める。
fn read_last_summarized_home_at(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_SUMMARIZED_HOME_AT))
}

/// `last-applied-rev` を読む（不存在/空なら `None`）。
fn read_last_applied_rev(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_APPLIED_REV))
}

/// home 部分適用が最後に確定した home スコープ pin を読む（不存在/空なら `None`）。
///
/// home-only catch-up（`update home`・非 defer）の dedup 判定（[`should_switch_home`]）で使う。marker が無い
/// （home-only 適用が一度も成功していない・本機能導入前）なら `None` で、その場合 home-only は switch する
/// （全体適用の `last-applied-rev` が一致しない限り）。
fn read_last_applied_home_rev(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_APPLIED_HOME_REV))
}

/// 最後に `--full` 適用した `flake.lock` 全体の identity を読む（不存在/空なら `None`）。
///
/// `--full` の switch 要否判定で、dotfiles pin 不変でも lock 全体が変化したかを比較するために使う
/// （finding 3368636842）。marker が無い（`--full` 初回・本機能導入前）なら `None` で、その場合 `--full` は
/// 必ず switch する（lock 全体を未適用とみなす）。
fn read_last_applied_lock_id(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_APPLIED_LOCK_ID))
}

/// home 部分適用が最後に `--full` 適用した `flake.lock` 全体 identity を読む（不存在/空なら `None`）。
///
/// `update home --full` の switch 要否判定（[`should_switch_home_full`]）で、dotfiles pin 不変でも lock 全体が
/// 変化したかを home スコープで比較するために使う（finding 3376248543）。marker が無い（home `--full` 初回・本機能
/// 導入前）なら `None` で、その場合 home `--full` は必ず switch する（lock 全体を未適用とみなす）。全体スコープの
/// `last-applied-lock-id` とは分離した home 専用カーソルであり、全体 `--full` 適用の dedup を動かさない。
fn read_last_applied_home_lock_id(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(LAST_APPLIED_HOME_LOCK_ID))
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

/// defer 時に控えた「このサイクルの token」を読む（不存在/空なら `None`）。
///
/// commit はこの token が `--rev-marker-token` の値と一致する時だけ `deferred-rev` を確定する。darwin 実行中に
/// 別サイクルが `deferred-rev`/`deferred-token` を上書きすれば token が変わるため、root のサイクルの commit は
/// 不一致を検知して未適用 pin を確定しない（finding 3368519975）。
fn read_deferred_token(state_dir: &Path) -> Result<Option<String>> {
    read_trimmed_rev(&state_dir.join(DEFERRED_TOKEN))
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

/// home 部分適用が適用した dotfiles repo pin を home スコープ marker へ原子的に控える（ユーザ所有）。
///
/// home-only catch-up（`update home`・非 defer）の適用成功後に確定し、次回 home-only の [`should_switch_home`]
/// が同一 pin を dedup できるようにする（finding 3374863446）。home スコープしか確定しないため、全体適用が読む
/// `last-applied-rev` を動かさず darwin を starve させない。`--dry-run` では書かない。
fn write_last_applied_home_rev(state_dir: &Path, rev: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(LAST_APPLIED_HOME_REV), rev, dry_run)
}

/// 最後に適用成功した時点の nixpkgs rev を原子的に書き込む（ユーザ所有）。`--dry-run` では書かない。
///
/// `last-applied-rev` と同時に確定し、catch-up 要約 span の真の起点（未適用範囲の起点）として次回実行で読む。
fn write_last_applied_nixpkgs_rev(state_dir: &Path, rev: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(LAST_APPLIED_NIXPKGS_REV), rev, dry_run)
}

/// `--full` 適用した `flake.lock` 全体の identity を原子的に書き込む（ユーザ所有）。`--dry-run` では書かない。
///
/// `--full` の全体適用成功時にだけ確定し、次回 `--full` 実行で lock 全体の変化（dotfiles pin 不変でも他 input が
/// 動いたケース）を検知する基準にする（finding 3368636842）。非 `--full` 経路は pin ベース判定を維持するため
/// 本 marker を書かない（dotfiles input だけの更新は pin が代表する）。
fn write_last_applied_lock_id(state_dir: &Path, lock_id: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(LAST_APPLIED_LOCK_ID), lock_id, dry_run)
}

/// home 部分適用が `--full` 適用した `flake.lock` 全体 identity を home スコープ marker へ原子的に控える（ユーザ所有）。
///
/// `update home --full`（非 defer）の適用成功後に確定し、次回 home `--full` の [`should_switch_home_full`] が
/// 同一 lock を dedup できるようにする（finding 3376248543）。全体スコープの `last-applied-lock-id` を動かさないため、
/// 全体 `--full` 適用（target=all）の dedup を壊さず darwin を starve させない。`--dry-run` では書かない。
fn write_last_applied_home_lock_id(state_dir: &Path, lock_id: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(LAST_APPLIED_HOME_LOCK_ID), lock_id, dry_run)
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

/// defer 時に「このサイクルの token」を原子的に控える（ユーザ所有）。`--dry-run` では書かない。
///
/// `deferred-rev`/`deferred-nixpkgs-rev` と同じ瞬間に書き、commit が token 一致を検証して **このサイクルで
/// 適用した pin だけ**を確定できるようにする（finding 3368519975）。
fn write_deferred_token(state_dir: &Path, token: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(DEFERRED_TOKEN), token, dry_run)
}

/// deferred marker（`deferred-rev`/`deferred-nixpkgs-rev`/`deferred-token`）を消す。`--dry-run` では触らない。
///
/// 新サイクルの冒頭（defer 書込み前）と commit 確定後の両方で呼び、deferred 値を 1 サイクルへ閉じる。これにより
/// commit が読む deferred 値は **そのサイクルの defer ステップが書いた値だけ**になり、darwin 失敗等で commit へ
/// 到達しなかった前サイクルの残骸を、後続サイクルの commit が未適用 pin として誤確定しない（サイクルローカル化）。
/// token も pin/nixpkgs rev と同じサイクルへ閉じる（commit の token 一致検証が別サイクルの token を拾わないため）。
/// 不存在の marker 除去は no-op（致命にしない）。
fn clear_deferred_markers(state_dir: &Path, dry_run: bool) {
    if dry_run {
        return;
    }
    let _ = fs::remove_file(state_dir.join(DEFERRED_REV));
    let _ = fs::remove_file(state_dir.join(DEFERRED_NIXPKGS_REV));
    let _ = fs::remove_file(state_dir.join(DEFERRED_TOKEN));
}

/// 最後に要約を表示/追記し終えた履歴エントリの `at` を原子的に書き込む（ユーザ所有）。`--dry-run` 不書込。
///
/// 要約「後」に書くことで、次回 present_summary の span 起点（`after_at`）が「最後に見せ終えたエントリの `at`」
/// になり、同日二重追記の抑止（A）・brew-only `N -> N` 再表示の抑止・partial-failure 堅牢性（B）を両立させる。
fn write_last_summarized_at(state_dir: &Path, at: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(LAST_SUMMARIZED_AT), at, dry_run)
}

/// home-only NixOnly 要約専用の要約済み `at` カーソルを原子的に書き込む（ユーザ所有）。`--dry-run` 不書込。
///
/// `All` スコープの `last-summarized-at` と分離した home スコープ専用カーソルを進めることで、daemon 端末で
/// home-only NixOnly 要約が `All` スコープのカーソルを cask エントリ越しに進め、commit `All` 要約の cask を
/// starve させる退行を防ぐ（運用整合 finding 是正）。要約「後」に書く点は `last-summarized-at` と同じ。
fn write_last_summarized_home_at(state_dir: &Path, at: &str, dry_run: bool) -> Result<()> {
    write_rev_atomic(&state_dir.join(LAST_SUMMARIZED_HOME_AT), at, dry_run)
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

/// 適用後要約の scope。**source_filter と要約済み `at` カーソルを 1 組に束ねる**ことで、read/write のカーソルが
/// scope 間で食い違わないようにする（運用整合 finding 是正の核）。
///
/// - `All`: 全体適用（target=all・非 defer）と daemon commit step（`--commit-rev-marker`）。nix + cask を要約し、
///   `last-summarized-at` を読み書きする。
/// - `HomeOnlyNix`: home-only catch-up（`update home`・非 defer。zsh ログイン catch-up）。nix（NixOnly）だけを
///   要約し、**`All` とは分離した `last-summarized-home-at`** を読み書きする。
///
/// scope を分離する理由: home-only NixOnly 要約が `select_entries_after` の filter「前」の選択 span 終端 `at` まで
/// カーソルを進めるため、共有カーソル（`last-summarized-at`）を使うと、daemon 端末で home-only 要約が cask エントリ
/// 越しに `All` スコープのカーソルを進め、commit `All` 要約が空 span になって **実適用 cask が starve** する。scope
/// 別カーソルにすれば、home-only NixOnly 要約は `All` スコープのカーソルを動かさず、commit `All` 要約が cask を
/// 必ず 1 回要約できる（要件1）。home-only 専用端末では NixOnly 要約だけが home カーソルを進め、nix の show-once を
/// 維持しつつ cask を「適用済み」と誤表示しない（要件2）。
#[derive(Clone, Copy)]
enum SummaryScope {
    /// 全体適用 / daemon commit step。nix + cask を `last-summarized-at` カーソルで要約する。
    All,
    /// home-only catch-up（NixOnly）。nix だけを `last-summarized-home-at` カーソルで要約する。
    HomeOnlyNix,
}

impl SummaryScope {
    /// この scope の要約に使う出所フィルタ（`All` は nix + cask、`HomeOnlyNix` は nix のみ）。
    fn source_filter(self) -> update_history::domain::wire::PackageSourceFilter {
        match self {
            SummaryScope::All => update_history::domain::wire::PackageSourceFilter::All,
            SummaryScope::HomeOnlyNix => update_history::domain::wire::PackageSourceFilter::NixOnly,
        }
    }

    /// この scope の要約済み `at` カーソルを読む（span 起点）。scope 別ファイルから読む。
    fn read_cursor(self, state_dir: &Path) -> Result<Option<String>> {
        match self {
            SummaryScope::All => read_last_summarized_at(state_dir),
            SummaryScope::HomeOnlyNix => read_last_summarized_home_at(state_dir),
        }
    }

    /// この scope の要約済み `at` カーソルを終端 `at` へ進める（要約成功後に書く）。scope 別ファイルへ書く。
    fn write_cursor(self, state_dir: &Path, at: &str, dry_run: bool) -> Result<()> {
        match self {
            SummaryScope::All => write_last_summarized_at(state_dir, at, dry_run),
            SummaryScope::HomeOnlyNix => write_last_summarized_home_at(state_dir, at, dry_run),
        }
    }
}

/// 適用後要約を表示/追記し、**成功時に** scope の要約済み marker を終端 `at` へ進める。
///
/// span 起点（`after_at`）と書き戻す marker は **同一 scope のカーソル**（`SummaryScope::read_cursor` /
/// `write_cursor`）であり、`All` は `last-summarized-at`、`HomeOnlyNix` は `last-summarized-home-at` を読み書きする。
/// read と write を同じ scope のカーソルへ束ねることで、home-only NixOnly 要約が `All` スコープのカーソルを cask
/// エントリ越しに進めて commit `All` 要約の cask を starve させる退行を防ぐ（運用整合 finding 是正）。
///
/// `present_summary`（表示/pending 追記）を呼び、空でない span を要約し終えたら終端 `at` を scope カーソルへ書く。
/// 要約「後」に marker を進めることで、partial-failure（要約中の異常終了）で未表示範囲を失わず、同日に
/// defer/commit 経路が連続しても 2 回目は起点 = 終端 `at` → 空 span → 再追記しない。`present_summary` が `Err`
/// （履歴 TOML 破損・pending-summary 書込み失敗等）なら marker を進めず `Err` を伝播する（呼び出し側が
/// 要約失敗時に apply-dedup marker を確定しないことで、未表示 span を次回再 switch で再要約できる）。
/// 空 span（`None`）のときは marker を進めない（前回 `at` を保つ）。tty 判定はここで 1 回だけ解決して
/// `present_summary` へ注入する（テストが tty 経路へ誤って入らないようアンビエント大域依存を呼び出し側へ集約）。
fn present_and_commit_summary(state_dir: &Path, scope: SummaryScope, dry_run: bool) -> Result<()> {
    let span_start_at = scope.read_cursor(state_dir)?;
    let stdout_is_terminal = std::io::stdout().is_terminal();
    let summarized_at = present_summary(
        state_dir,
        span_start_at.as_deref(),
        scope.source_filter(),
        dry_run,
        stdout_is_terminal,
    )?;
    if let Some(at) = summarized_at {
        scope.write_cursor(state_dir, &at, dry_run)?;
    }
    Ok(())
}

/// apply-dedup marker（`last-applied-*` / home スコープ marker）または defer marker を確定する（要約成功後に呼ぶ）。
///
/// repo pin 全体の確定（`last-applied-rev`/`last-applied-nixpkgs-rev`/`last-applied-lock-id`）は
/// **全体適用（target=all・非 defer）でのみ**行う。部分 target で全体 pin を確定すると、適用していない他 target
/// がその rev について以降 skip され（`should_switch` が前回値一致で skip）、未適用のまま starve するためである。
///
/// 一方、home 部分適用（`update home`・非 defer。zsh login catch-up）は **home スコープ marker
/// （`last-applied-home-rev`）** を確定する（finding 3374863446 の退行是正）。全体 marker を動かさないので darwin は
/// starve しないが、home スコープでは同一 pin の再適用を dedup できる（[`should_switch_home`] が読む）。home marker
/// は home スコープしか進めないため、全体適用の `should_switch` 判定とは独立しており、相互に干渉しない。
///
/// `--defer-rev-marker`（daemon home step）では `last-applied-*` も home marker も確定せず、適用した pin/nixpkgs rev/
/// サイクル token を defer marker へ控える（後続の `--commit-rev-marker` が確定する）。
///
/// caller responsibility: 要約（`present_and_commit_summary`）が成功した後に呼ぶこと（finding 3368519980）。
/// 要約より先に last-applied を確定すると、要約だけが失敗した場合に次回 `should_switch` が同一 pin で早期 return し
/// 未表示 span が二度と要約されない。要約成功後に確定すれば、要約失敗時は last-applied を書かず次回再 switch で
/// 未表示 span を再要約できる。
///
/// `history_synced`: 履歴複製（`sync_history`）が成功したか。**非 defer の apply-dedup marker は
/// `history_synced == true` のときだけ確定する**（finding 3376248509）。一時的な archive/network 失敗で
/// `history_synced == false` のとき確定すると、次回同一 pin で `sync_history` より前に早期 return し、通信復旧後も
/// 同じ pin で履歴複製・適用後要約を再試行できず、次の bump まで show/pending summary が空/古いままになる。よって
/// 履歴同期未成功時は確定せず、次回同一 pin で switch（冪等）→ 再同期 → 再要約を試せるようにする。defer 経路の
/// deferred marker は要約を commit step へ委譲するため `history_synced` に依らず控える（commit が要約成功後に確定し、
/// 失敗時は次サイクルで再試行する＝finding 3376248504）。
fn commit_apply_markers(
    state_dir: &Path,
    options: &UpdateOptions,
    config_dir: &Path,
    current_pin: &str,
    applied_nixpkgs_rev: &str,
    history_synced: bool,
    dry_run: bool,
) -> Result<()> {
    if !options.defer_rev_marker && options.switch.is_full_apply() && history_synced {
        write_last_applied_rev(state_dir, current_pin, dry_run)?;
        // dotfiles pin と同時に、今回適用した nixpkgs rev も確定する。defer 時は rev 未確定のため書かない
        // （darwin 成功後の `--commit-rev-marker` がまとめて確定する）。
        write_last_applied_nixpkgs_rev(state_dir, applied_nixpkgs_rev, dry_run)?;
        // `--full` の全体適用では lock 全体 identity も確定し、次回 `--full` で他 input の変化を検知する基準にする
        // （finding 3368636842）。非 `--full` 経路は pin が代表するので lock-id marker は書かない。
        if options.full {
            let applied_lock_id = read_lock_id(config_dir)?;
            write_last_applied_lock_id(state_dir, &applied_lock_id, dry_run)?;
        }
    } else if !options.defer_rev_marker && options.switch.is_home_only_apply() && history_synced {
        // home 部分適用（zsh login catch-up）: 全体 marker は確定せず（darwin starve 回避）、home スコープ marker
        // だけ確定する（finding 3374863446）。これで次回 home-only は同一 pin を dedup でき、毎ログイン再適用の
        // 無限ループを止める。home スコープしか進めないため darwin は引き続き適用要と判定される。nixpkgs rev /
        // nixpkgs rev / 全体 lock-id は全体スコープ marker なのでここでは確定しない（home-only は cask/system を
        // 適用しない）。
        write_last_applied_home_rev(state_dir, current_pin, dry_run)?;
        // `update home --full` では home スコープの lock-id も確定し、次回 home `--full` で dotfiles pin 不変 +
        // 他 input 変化のケースを検知できるようにする（finding 3376248543）。全体スコープの `last-applied-lock-id` は
        // 動かさないため、全体 `--full` 適用（target=all）の dedup を壊さず darwin を starve させない。非 `--full` の
        // home-only は pin が代表するので home lock-id marker を書かない。
        if options.full {
            let applied_lock_id = read_lock_id(config_dir)?;
            write_last_applied_home_lock_id(state_dir, &applied_lock_id, dry_run)?;
        }
    } else if options.defer_rev_marker {
        // defer 経路: `last-applied-*` はまだ確定しないが、**この時点で適用した pin / nixpkgs rev** を defer
        // marker へ控える（B）。後続の `--commit-rev-marker` はこの defer 値を確定し、commit 時に現在 pin を
        // 読み直さない。これにより home 適用後・commit 前に lock が再 bump されても、適用した pin と確定する pin
        // が必ず一致し、適用していない pin を `last-applied` へ確定する乖離を防ぐ。
        write_deferred_rev(state_dir, current_pin, dry_run)?;
        write_deferred_nixpkgs_rev(state_dir, applied_nixpkgs_rev, dry_run)?;
        // ラッパーから渡された **サイクル token** を deferred 値と同じ瞬間に控える（finding 3368519975）。commit は
        // この token と `--rev-marker-token` の一致を検証して、このサイクルで適用した pin だけを確定する。darwin
        // 実行中（user lock 解放後）に別サイクルが deferred 値を上書きすれば token も変わり、root のサイクルの
        // commit は不一致を検知して未適用 pin を確定しない。token 未指定（旧ラッパー）なら控えず、commit も
        // 従来の後方互換縮退に倒れる。
        if let Some(token) = options.rev_marker_token.as_deref() {
            write_deferred_token(state_dir, token, dry_run)?;
        }
    }
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
///
/// `source_filter` は実際に適用した target に対応する出所だけへ要約を絞る（finding 3368653947）。home 部分適用は
/// `NixOnly` で brew cask を除外し、全体/darwin 適用は `All`。tty / 非 tty / dry-run の全描画経路へ同じ filter を
/// 渡し、どの経路でも未適用 cask を通知しない。
fn present_summary(
    state_dir: &Path,
    summarized_after_at: Option<&str>,
    source_filter: update_history::domain::wire::PackageSourceFilter,
    dry_run: bool,
    stdout_is_terminal: bool,
) -> Result<Option<String>> {
    // 履歴は state dir のローカル複製（`<state-dir>/history`）から読む。`~/.config/dotfiles` には更新履歴が
    // 無く、適用時に input source から複製済みのこの dir を offline・決定論で参照する。
    let source = state_dir.join(HISTORY_LOCAL_SUBDIR);

    if stdout_is_terminal {
        // tty: 起動元端末へ直接表示。stdout を sink にして show 描画を再利用する。要約し終えた終端 `at` を返す。
        let summarized_at = update_history::render_applied_summary(
            &source,
            summarized_after_at,
            source_filter,
            std::io::stdout(),
        )?;
        if !dry_run {
            append_last_run_log(state_dir, summarized_after_at, source_filter)?;
        }
        return Ok(summarized_at);
    }

    // 非 tty（background）: pending-summary へ追記し、次回シェルが 1 回だけ消費する。
    if dry_run {
        // dry-run でもファイル契約を観測できるよう、捕捉バッファへ描画して破棄する（副作用なし）。
        let mut buffer = Vec::new();
        let summarized_at = update_history::render_applied_summary(
            &source,
            summarized_after_at,
            source_filter,
            &mut buffer,
        )?;
        return Ok(summarized_at);
    }
    let summarized_at =
        append_pending_summary(state_dir, &source, summarized_after_at, source_filter)?;
    append_last_run_log(state_dir, summarized_after_at, source_filter)?;
    Ok(summarized_at)
}

/// `pending-summary` へ適用要約ブロックを追記公開する（上書きしない）。完成済みブロックだけを公開する（C）。
///
/// 非 tty 適用ごとに 1 ブロックを足す。daemon が連続適用しても未表示 rev を失わないよう累積で運用し、消費
/// （表示と削除）は zsh フック（`config/zsh/auto-update.zsh`）が原子的 rename で 1 回だけ行うファイル契約とする。
///
/// **render 途中失敗の非公開（C）**: render は buffer 上で完成させてから公開するため、履歴 source が壊れて render
/// に失敗しても `pending-summary` には 1 バイトも触れない（部分ブロックを公開・消費させない）。
///
/// **consumer との rename 競合の回避（finding 3368519974）**: 旧実装は `pending-summary` を `append` open して
/// から `write_all` するまでの間も live パスを保持していた。その隙に consumer（zsh）が `mv "$pending"
/// "$pending.consuming.$$"` で消費すると、writer は rename 済みの孤児 inode へ新ブロックを書き、consumer が
/// 既に `cat` 済みなら以後 `rm` されて要約が失われた。これを防ぐため、producer も **consumer と同じ rename による
/// 所有権獲得**で publish する: (1) 既存 `pending-summary` を `pending-summary.appending.<pid>` へ atomic rename
/// して所有権を取る（consumer が先に `mv` 済みなら NotFound → 既存ブロック無しで新規公開に倒れる。consumer が
/// 取った既存ブロックは consumer 側で表示されるので失われない）、(2) 取得した既存内容に新ブロックを連結して
/// temp に完成させる、(3) temp を `pending-summary` へ atomic rename で publish する。`append` open の窓が無く、
/// publish は単一の atomic rename なので、consumer はどの瞬間も「完全な旧 pending」か「完全な新 pending」か
/// 「(producer が取得中の)不在」のいずれかだけを観測し、孤児 inode への書込みで要約を失わない。
/// claim/temp/publish は同一 dir 内に置き、rename が原子的に成立することを前提にする。失敗時は temp/claim を
/// 掃除し、可能なら claim した既存内容を `pending-summary` へ戻して既存ブロックを失わない。
fn append_pending_summary(
    state_dir: &Path,
    source: &Path,
    summarized_after_at: Option<&str>,
    source_filter: update_history::domain::wire::PackageSourceFilter,
) -> Result<Option<String>> {
    let path = state_dir.join(PENDING_SUMMARY);
    // render は buffer 上で行い、要約し終えた終端 `at`（次回カーソル）も同時に得る。render 失敗は `pending-summary`
    // へ波及させず（live ファイルへ一切触れない）early return する（C）。`source_filter` で適用 target に対応する
    // 出所だけへ絞る（home 部分適用は nix のみ）。
    let mut rendered = Vec::new();
    let summarized_at = update_history::render_applied_summary(
        source,
        summarized_after_at,
        source_filter,
        &mut rendered,
    )?;

    // (1) 既存 `pending-summary` を claim ファイルへ atomic rename して所有権を取る。consumer が先に `mv` 済み
    //     なら NotFound（= 既存ブロック無し）。これ以外の I/O 失敗は伝播する。
    let claim_path = path.with_file_name(format!(
        "{PENDING_SUMMARY}.appending.{}",
        std::process::id()
    ));
    let _ = fs::remove_file(&claim_path); // 前回中断の残骸を掃除してから claim する。
    let existing = match fs::rename(&path, &claim_path) {
        Ok(()) => fs::read(&claim_path)
            .with_context(|| format!("failed to read claimed {}", claim_path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            return Err(
                anyhow::Error::from(error).context(format!("failed to claim {}", path.display()))
            );
        }
    };

    // (2) 既存内容 + 新ブロックを temp に完成させ、(3) temp を `pending-summary` へ atomic rename で publish する。
    let temp_path = path.with_file_name(format!(
        "{PENDING_SUMMARY}.publish.{}.tmp",
        std::process::id()
    ));
    let publish = (|| -> Result<()> {
        let mut combined = existing.clone();
        combined.extend_from_slice(&rendered);
        fs::write(&temp_path, &combined)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        fs::rename(&temp_path, &path)
            .with_context(|| format!("failed to publish {}", path.display()))?;
        Ok(())
    })();

    match publish {
        Ok(()) => {
            // publish 成功。claim ファイルはもう不要（内容は temp 経由で publish 済み）。
            let _ = fs::remove_file(&claim_path);
            Ok(summarized_at)
        }
        Err(error) => {
            // publish 失敗。temp を掃除し、claim した既存内容を `pending-summary` へ戻して既存ブロックを失わない
            // （新ブロックは次回再試行で再 render される。要約 marker は呼び出し側が未確定にする）。
            let _ = fs::remove_file(&temp_path);
            if !existing.is_empty() {
                let _ = fs::rename(&claim_path, &path);
            } else {
                let _ = fs::remove_file(&claim_path);
            }
            Err(error)
        }
    }
}

/// `last-run.log` へ適用要約を残す（適用経路の出力記録）。
///
/// 適用の人間可読な要約を上書き保存し、直近 1 回分の適用内容を後から確認できるようにする。履歴 source は
/// state dir のローカル複製（`<state-dir>/history`）を読む。`source_filter` で適用 target に対応する出所だけを
/// 記録する（home 部分適用は nix のみ。pending-summary 表示と同じ範囲を log にも残す）。要約済み marker の確定は
/// 呼び出し側（`present_summary` の戻り値経由）が担うため、本関数は render 戻り値（終端 `at`）を破棄する。
fn append_last_run_log(
    state_dir: &Path,
    summarized_after_at: Option<&str>,
    source_filter: update_history::domain::wire::PackageSourceFilter,
) -> Result<()> {
    let path = state_dir.join(LAST_RUN_LOG);
    let source = state_dir.join(HISTORY_LOCAL_SUBDIR);
    let file =
        fs::File::create(&path).with_context(|| format!("failed to create {}", path.display()))?;
    let _ =
        update_history::render_applied_summary(&source, summarized_after_at, source_filter, &file)?;
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
/// 奪取権を直列化する `update.lock.steal` は、奪取区間（古い lock を直接 remove せず **必ず rename で退避してから**
/// 新 lock を create_new で張る rename-CAS）の **間だけ**存在する短命 marker である。区間中にプロセスが
/// kill/OOM/電源断/abort されると `remove_file` が走らず marker が恒久残骸化し、以後 `steal_stale_lock` が必ず
/// `AlreadyExists` で `None`（skip）へ倒れ、**stale lock の奪取が永久に起きなくなる**（marker 残骸が「stale lock を
/// 永久 skip しない」という機構自体の目的を破る）。これを防ぐため marker 自身に短い TTL を与え、TTL より古い marker
/// は孤児とみなして回収し奪取権を再取得する。奪取区間（rename 退避 + create_new、I/O 数回）は秒オーダーで完了する
/// ため、誤回収（実行中の別プロセスの奪取権を横取り）を避けつつ孤児を速やかに掃除できる短さにする。5 分は実奪取区間より十分長く、
/// `LOCK_STALE_SECS` より十分短い。
const STEAL_MARKER_STALE_SECS: u64 = 5 * 60;

/// rename ベース CAS の中継ファイル名を**プロセス内でも一意**にするための単調カウンタ。
///
/// 奪取権の rename CAS（[`UpdateLock::reclaim_stale_steal_marker`]）と stale lock の rename CAS
/// （[`UpdateLock::steal_stale_lock_file_via_rename`]）は孤児を一意名へ rename して「src 単一 → 勝者 1 人」を
/// 成立させる。中継名を `pid + epoch秒` だけで作ると、**同一プロセスの複数スレッドが同一秒に**回収を試みた場合に
/// 中継名が衝突し、複数スレッドが同じ dst へ rename して
/// 全員「成功（上書き）」と誤判定する（CAS が崩れて二重奪取）。プロセス内一意のカウンタを足して中継名を必ず
/// 別名にし、各回収者の rename dst を衝突させない（マルチスレッドでも src 単一性で 1 人に絞る）。
static STEAL_RENAME_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// **同一プロセス内の奪取区間を直列化する process-wide mutex（finding 3368585963 の単一勝者厳密化）**。
///
/// 実運用では `dotfiles update` は 1 マシン 1 プロセスで、奪取区間に入るのは常に 1 スレッドだけ（プロセス内で
/// 自分自身と奪取を競うことはない）。プロセス間の排他は `update.lock` / `update.lock.steal` のファイル `O_EXCL`
/// が担う。一方で **複数スレッドが同一プロセス内で同時に奪取区間へ入る**と、ある世代の lock を別スレッドが
/// rename 退避して `lock_path` が一瞬空く窓に、同プロセスの `try_acquire` 冒頭 `create_new`(O_EXCL) が割り込んで
/// **同一世代でない 2 本目の lock を取得**し、退避された側が phantom holder として残る（同時保持 2）。これは
/// file `O_EXCL` だけでは塞げない（O_EXCL は「不在 → 作成」の同時実行しか直列化せず、退避で生じた不在窓への
/// 割り込みは別経路の正規取得に見える）。そこで **奪取区間全体をプロセス内 mutex で 1 スレッドに直列化**し、
/// 「ある世代の fresh lock を別スレッドが退避して不在窓を作る」cross-generation rename を原理的に消す。これにより
/// 奪取は常に「stale 世代 1 本 → （奪取者 or 不在窓を取った try_acquire）1 人」の単一遷移になり、同時保持は
/// 高々 1 本になる。プロセス間は従来どおりファイル marker / lock の `O_EXCL` が担うため、本 mutex はプロセス内
/// 直列化を足すだけで cross-process 排他を弱めない。`libc` を直呼びせず std の `Mutex` のみで実現する。
static STEAL_SECTION_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// rename CAS 中継ファイルの一意 suffix（`<pid>.<epoch>.<seq>`）を作る。
///
/// `pid`（プロセス間一意）+ `epoch秒`（時間）+ プロセス内単調 `seq`（[`STEAL_RENAME_SEQ`]、同一秒の同一プロセス
/// 内スレッド衝突回避）で、奪取中継ファイル名を全 caller で別名にする。rename CAS の「src 単一 → 勝者 1 人」を
/// マルチスレッドでも崩さないための一意化であり、中身の値の意味はない（衝突回避だけが目的）。
fn steal_rename_suffix() -> String {
    let seq = STEAL_RENAME_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}.{}.{}", std::process::id(), now_epoch_secs(), seq)
}

/// `update.lock` の `O_EXCL` ベース排他ロック。drop でロックファイルを除去する。
///
/// flock(2) を使うと `libc` 直呼び（禁止）か新規 crate が要るため、移植性とテスト容易性を優先し
/// `create_new`（`O_CREAT|O_EXCL`）でロックファイルを作る方式を採る。作成成功＝ロック取得、`AlreadyExists`＝
/// 取得失敗だが、**stale lock（プロセス kill/再起動で `Drop` 未実行のまま残った孤児）を永久 skip しない**よう、
/// 既存 lock の owner 生存（pid + プロセス開始時刻 identity）と timestamp を見て、孤児（[`LOCK_STALE_SECS`] 超過）
/// なら奪取する。lock ファイルはユーザ所有 state dir 配下に `pid\nepoch_secs\nstart_token`（[`lock_payload`]）で書き、
/// drop で除去する。`--dry-run` では実ロックファイルを作らず（副作用なし）、常に取得成功として判定経路を通す。
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

    /// stale（孤児）lock を **lock ファイル自身の rename ベース CAS** で奪取する。勝者だけが新 lock を張る。
    ///
    /// **race-free CAS の要点（finding 3368585963）**: 奪取の本体（古い lock の除去 → 新 lock 作成）を「無条件
    /// `remove_file` → `create_new`」で複数プロセスにやらせると、「A が新 lock を張った直後に B の `remove_file` が
    /// **A の新 lock を消し**、B も `create_new` に成功して A・B 双方が奪取に成立」する remove-clobber 二重奪取が
    /// 起きる（`create_new` の O_EXCL は同時 create しか直列化せず、先行 create を後続 remove が消すのは防げない）。
    /// 旧実装は別ファイル（`update.lock.steal`）の O_EXCL を奪取権 CAS に使ったが、その marker を奪取区間終了で
    /// 除去する設計上、孤児 marker の TTL 回収経路や marker 再取得窓で複数プロセスが同時に奪取区間へ入る穴が残った。
    ///
    /// 本実装は **奪取権 marker（`update.lock.steal`）の取得**で奪取区間への入場を直列化しつつ、奪取区間内の **実
    /// lock 張替えも lock ファイル自身の rename ベース CAS** にして二段で単一勝者化する。marker の取得経路は
    /// [`claim_steal_marker`]（newcomer の `create_new`(O_EXCL) と、孤児 marker の TTL 回収を rename CAS で単一化する
    /// 経路）。marker を取れた者だけが奪取区間に入り、区間内で stale lock を一意名へ rename 退避してから
    /// `create_new`(O_EXCL) で新 lock を張る（[`steal_within_marker`]）。**lock path を直接 remove せず必ず rename で
    /// 退避してから消す**ため、marker 経路が万一同時入場を許しても実 lock の取得点は唯一 `create_new`(O_EXCL) に
    /// 集約され、remove-clobber 二重奪取が原理的に起きない。区間終了時に marker を必ず除去する。`libc` を直呼びせず
    /// std の create_new / rename のみで実現する。
    fn steal_stale_lock(path: &Path) -> Result<Option<Self>> {
        // **プロセス内の奪取区間を 1 スレッドへ直列化する**（[`STEAL_SECTION_MUTEX`]）。複数スレッドが同時に奪取区間
        // へ入ると、ある世代の lock を別スレッドが rename 退避して `lock_path` が空く窓に、同プロセスの別経路
        // `create_new`(O_EXCL) が割り込んで同時保持 2 本になりうる（cross-generation rename）。区間全体を mutex で
        // 直列化してこの窓を消す。実運用は 1 プロセス 1 スレッドなので競合は無く、プロセス間は file `O_EXCL` が担う。
        let _section = STEAL_SECTION_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // 奪取区間の **ファイル protocol 本体**（marker CAS → 実 lock 張替え → marker 除去）。protocol そのものが
        // create_new(O_EXCL)/rename CAS で cross-process 単一勝者を保証するため、in-process mutex は補助に過ぎない。
        Self::steal_stale_lock_protocol(path)
    }

    /// 奪取区間のファイル protocol 本体（marker CAS → 実 lock 張替え → marker 除去）。
    ///
    /// `STEAL_SECTION_MUTEX` を **持たない**（呼び出し側 [`steal_stale_lock`] が補助的に取る）。この protocol 自体が
    /// `create_new`(O_EXCL) と rename CAS だけで **cross-process（= 別プロセス間。in-process mutex は効かない）でも
    /// 単一勝者を保証する**ことが単独要件であり、テストはこの関数を mutex を経由せず多スレッドから直接競わせて
    /// 「rename-CAS を旧バグ（lock path を直接 remove→create_new）へ退行させると同時保持が 2 以上になる」ことを
    /// 固定する（finding 3368585963 / B 是正）。in-process mutex はこの単一勝者性の上の冗長な直列化であり、
    /// protocol の正しさには依存しない。`libc` を直呼びせず std の create_new / rename のみで実現する。
    fn steal_stale_lock_protocol(path: &Path) -> Result<Option<Self>> {
        let steal_marker = path.with_file_name(format!("{LOCK_FILE}.steal"));
        // 奪取権 CAS: marker を取得できた者だけが奪取区間に入る（孤児 marker は rename CAS で単一回収）。
        if !Self::claim_steal_marker(&steal_marker)? {
            // 別プロセスが奪取区間中（marker 新鮮）か、孤児回収を別プロセスに奪われた。古い lock に触れず skip。
            return Ok(None);
        }
        // marker により直列化された奪取区間。実 lock 張替えも rename CAS で単一化する。終了時に marker を除去する。
        //
        // **marker の除去は奪取結果が確定した後に行う**（順序が単一勝者保証の要）。除去を steal の「前」や並行に
        // 出すと、reclaim 経路の敗者が走らせる `create_new_marker`（[`claim_steal_marker`] ステップ 3）が、
        // 本 owner の steal 区間中に marker を再取得して **2 本目の奪取区間へ同時入場**しうる。除去を steal 完了後に
        // 限定し、かつ実 lock の取得点を `steal_stale_lock_file_via_rename` 内の唯一の `create_new`(O_EXCL) に
        // 集約することで、marker 経路が万一同時入場を許しても実 lock を握れるのは高々 1 人になる。
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
    /// 既存 marker の timestamp を見て [`STEAL_MARKER_STALE_SECS`] より古ければ **孤児とみなして回収**する。
    ///
    /// **TTL 回収も rename ベース CAS で単一勝者化する（finding 3368585963）**: 孤児 marker の回収を
    /// 「無条件 `remove_file` → `create_new`」にすると、複数プロセスが同時に孤児を観測した場合に二重奪取が
    /// 起きる: プロセス A が孤児を remove → `create_new` で新 marker を張った直後、(A の判定前に孤児を観測して
    /// いた)プロセス B が `remove_file` で **A の新 marker を消し**、B も `create_new` に成功して **A・B 双方が
    /// 奪取区間へ入る**（B の remove が A の新 marker を巻き込む TOCTOU で、`create_new` の `O_EXCL` は同時 create
    /// しか直列化せず、先行 create を後続 remove が消すのは防げない）。
    ///
    /// **奪取権を `steal_marker` の `create_new`（O_EXCL）に一本化し、孤児掃除は rename CAS で単一化する（単一勝者の
    /// 厳密化, finding 3368585963）**: 旧実装は孤児を「無条件 `remove_file` → `create_new`」で掃除したため、複数
    /// プロセスが同時に孤児を観測すると、A が remove → create_new で新 marker を張った直後に B の `remove_file` が
    /// **A の新 marker を消し**、B も create_new に成功して A・B 双方が奪取区間へ入る remove-clobber があった
    /// （`create_new` の O_EXCL は同時 create しか直列化せず、先行 create を後続 remove が消すのは防げない）。
    ///
    /// 本実装は **奪取権を `steal_marker` 1 ファイルの O_EXCL 所有に固定**し、孤児が居座る場合の掃除だけを rename CAS
    /// で単一化する。孤児を一意中継名（`update.lock.steal.reclaiming.<pid>.<epoch>.<seq>`）へ `fs::rename` で奪えた
    /// 1 人だけが孤児を除去し（rename は src 単一なので回収者は 1 人。中継経由で除去するので「他者の新 marker を
    /// remove で巻き込む」窓が無い）、除去後は **誰も奪取権を持たない状態**から `create_new`（O_EXCL）で奪取権を
    /// 競う。O_EXCL によりちょうど 1 人が `steal_marker` を取得して奪取区間に入る。掃除を逃した者・新鮮 marker を
    /// 観測した者は `false`（skip）へ倒れる。`libc` を直呼びせず std の create_new / rename のみで実現する。
    fn claim_steal_marker(steal_marker: &Path) -> Result<bool> {
        // ステップ 1: 奪取権 marker を O_EXCL で新規取得する（唯一の奪取権獲得点。ちょうど 1 人だけ成功）。
        if Self::create_new_marker(steal_marker)? {
            return Ok(true);
        }
        // 既存 marker あり。新鮮（実行中の別奪取者が保持）なら奪取権を譲る（横取りしない）。
        if !Self::steal_marker_is_stale(steal_marker) {
            return Ok(false);
        }
        // 孤児（TTL 超過）。rename CAS で 1 人だけが孤児を掃除する（奪取権はここでは取らない）。掃除後は誰も
        // marker を持たないので、続く create_new（O_EXCL）でちょうど 1 人が奪取権を取る。
        Self::reclaim_stale_steal_marker(steal_marker)?;
        // 孤児が掃除された後の正規取得を 1 回試みる。複数プロセスが同時に到達しても O_EXCL で 1 人だけ成功する。
        // 孤児がまだ残る（別プロセスの掃除が未完了・競合）なら AlreadyExists で `false` に倒れ、次回 try_acquire の
        // 再試行へ委ねる（無更新は安全側）。
        Self::create_new_marker(steal_marker)
    }

    /// 孤児（TTL 超過）と判定した steal marker を **rename ベース CAS** で 1 プロセスだけが掃除する（奪取権は取らない）。
    ///
    /// 孤児を一意中継名（`update.lock.steal.reclaiming.<pid>.<epoch>.<seq>`）へ `fs::rename` で奪う。POSIX rename は
    /// 同一 src を複数プロセスが別 dst へ rename しても **最初の 1 人だけ成功**し、残りは src 不在（`NotFound`）で
    /// 失敗する（src は 1 つしかない）。rename に成功した 1 プロセスだけが孤児の中身を再確認して除去する。除去は
    /// 中継ファイル経由で行うため、他プロセスが張り直した新 marker（別 path）を remove で巻き込まない（remove-clobber
    /// を塞ぐ）。
    ///
    /// **奪取権を張り直さず「掃除のみ」にする**: 掃除した本プロセスも含め、孤児除去後は誰も marker を持たないため、
    /// 続く `create_new`（[`claim_steal_marker`] のステップ 3、O_EXCL）でちょうど 1 人が奪取権を取る。掃除と奪取を
    /// 分離し、奪取権を唯一 O_EXCL に集約することで、孤児を rename で奪った後の二重奪取窓を断つ。
    ///
    /// 奪った中継ファイルは stale でも fresh でも **常に除去する**（元の `steal_marker` 位置へ戻さない）。fresh
    /// だった marker（判定〜rename 間に別勝者が張った）を戻すと、その owner が既に区間を終えて自分の marker を除去
    /// 済みのとき、戻した marker が誰にも掃除されず残骸化する。奪取権は marker の所有ではなく実 lock の rename CAS
    /// （[`steal_stale_lock_file_via_rename`]）が単一勝者を保証するため、fresh marker を消しても二重奪取は起きない
    /// （消された newcomer は実 lock を保持/取得中で、後続は fresh lock を観測して skip する）。rename 失敗
    /// （別プロセスが先に奪取・src 消滅）は何もせず戻る。中継名は [`steal_rename_suffix`] で一意化する。
    fn reclaim_stale_steal_marker(steal_marker: &Path) -> Result<()> {
        let reclaiming = steal_marker.with_file_name(format!(
            "{LOCK_FILE}.steal.reclaiming.{}",
            steal_rename_suffix()
        ));
        // 孤児 marker を一意名へ rename で奪う。src は 1 つなので同時掃除者のうち 1 人だけが成功する（CAS）。
        if let Err(error) = fs::rename(steal_marker, &reclaiming) {
            // 別プロセスが先に孤児を奪った（src 不在）か I/O エラー。前者は skip、後者は伝播。
            return match error.kind() {
                std::io::ErrorKind::NotFound => Ok(()),
                _ => Err(anyhow::Error::from(error).context(format!(
                    "failed to reclaim stale steal marker {}",
                    steal_marker.display()
                ))),
            };
        }
        // 奪った中身が真に孤児（stale）でも fresh でも、中継ファイルは **常に除去**する。fresh だった場合に中継を
        // 元の `steal_marker` 位置へ戻すと、その fresh marker の本来の owner（newcomer）が既に区間を終えて自分の
        // marker を除去済みのとき、戻した marker が誰にも掃除されず残骸化する（leftover）。奪取権は marker の所有では
        // なく実 lock の rename CAS（[`steal_stale_lock_file_via_rename`]、lock path を直接 remove せず rename 退避して
        // から create_new(O_EXCL)）が単一勝者を保証するため、fresh marker を消しても二重奪取は起きない（fresh marker
        // を消された newcomer は実 lock を既に保持/取得中で、後続は fresh lock を観測して skip する）。よって中継は
        // 戻さず破棄し、孤児除去後の `create_new`（[`claim_steal_marker`] ステップ 3、O_EXCL）に奪取権を集約する。
        let _ = fs::remove_file(&reclaiming);
        Ok(())
    }

    /// steal marker を `create_new`（`O_EXCL`）で作る。成功で `true`、既存（`AlreadyExists`）で `false`。
    ///
    /// 作成時は孤児回収（pid 生存 + 開始時刻 identity + TTL 判定）用に `pid\nepoch_secs\nstart_token`
    /// （[`current_lock_payload`]）を書く。書込み失敗は致命にしない（その場合の staleness 判定は保守的に「新鮮」へ
    /// 倒れ、誤回収を避ける）。
    fn create_new_marker(steal_marker: &Path) -> Result<bool> {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(steal_marker)
        {
            Ok(mut marker) => {
                let _ = write!(marker, "{}", current_lock_payload());
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(anyhow::Error::from(error).context(format!(
                "failed to create steal marker {}",
                steal_marker.display()
            ))),
        }
    }

    /// 既存 steal marker が孤児（TTL 超過）かを pid 生存 + timestamp で判定する。
    ///
    /// marker の payload pid が生存中なら（[`is_stale_lock_owner`]）TTL を超えていても新鮮（回収しない）に
    /// 倒し、実行中の別奪取者の奪取権を横取りしない。pid が消えた孤児（奪取区間中に kill / OOM / 電源断）か
    /// pid 解析不能のときだけ TTL（[`STEAL_MARKER_STALE_SECS`]）超過で孤児とみなして回収する。読取り失敗・
    /// marker 消滅は保守的に「新鮮（孤児でない）」へ倒す。
    fn steal_marker_is_stale(steal_marker: &Path) -> bool {
        let Ok(content) = fs::read_to_string(steal_marker) else {
            return false;
        };
        is_stale_lock_owner(&content, now_epoch_secs(), STEAL_MARKER_STALE_SECS)
    }

    /// 奪取権トークンを保持した奪取区間内で、現在の lock 状態に応じて新 lock を張る。
    ///
    /// 奪取権トークン（[`claim_steal_marker`]）取得待ちの間に別勝者が lock を更新した可能性があるため、取得
    /// 「後」に再判定する: lock 消滅なら新規作成、stale なら奪って張替え、fresh（別勝者が更新済み）なら奪取しない。
    ///
    /// **stale lock の張替えも rename ベース CAS で単一勝者化する**: 奪取権トークンは経路 2（孤児 marker 回収）で
    /// 複数の物理ファイル（`steal_marker` と中継名）に分かれうるため、奪取区間が原理上同時に 2 プロセスへ開く窓が
    /// ありうる。そこで実 lock の張替えも「無条件 `remove_file` → `create_new`」にせず、stale lock を一意中継名へ
    /// `fs::rename` で奪う CAS にする（src 単一 → 1 人だけ成功）。rename に成功した 1 人だけが旧 lock を消して
    /// `create_new` で新 lock を張り、敗者は `None`（skip）へ倒れる。これにより「A が新 lock を張った直後に B の
    /// remove が A の lock を消す」remove-clobber が原理的に起きず、奪取権トークン経路の如何に依らず実 lock の
    /// 取得者はちょうど 1 人になる。`libc` を直呼びせず std の create_new / rename のみで実現する。
    fn steal_within_marker(path: &Path) -> Result<Option<Self>> {
        // lock が在るなら fresh（別勝者が更新済み）か再確認し、fresh なら奪取しない。lock 消滅 or stale はいずれも
        // 下の rename CAS で奪取する（消滅は rename が NotFound → create_new で新規取得に倒れる）。
        if Self::lock_is_fresh(path) {
            return Ok(None);
        }
        Self::steal_stale_lock_file_via_rename(path)
    }

    /// 既存 lock が **生存中（fresh）**かを判定する。lock 不在・読取り不能・stale はいずれも `false`（奪取可）。
    ///
    /// 別勝者が奪取区間中に張り直した fresh lock（生存 pid）を後続が横取りしないための再確認。pid 生存 + timestamp
    /// 判定（[`is_stale_lock_owner`] の否定）で、生きた owner の lock だけを保護する。
    fn lock_is_fresh(path: &Path) -> bool {
        match fs::read_to_string(path) {
            Ok(content) => !is_stale_lock_owner(&content, now_epoch_secs(), LOCK_STALE_SECS),
            // lock 不在・読取り不能は fresh ではない（奪取/新規取得に倒す）。
            Err(_) => false,
        }
    }

    /// stale な実 lock ファイルを **rename ベース CAS** で 1 プロセスだけが奪い、新 lock を張る。
    ///
    /// stale lock を一意中継名（`update.lock.stealing.<pid>.<epoch>.<seq>`）へ `fs::rename` で奪う。src は 1 つ
    /// なので同時奪取者のうち 1 人だけが rename に成功する（残りは `NotFound`）。成功した 1 人だけが中継を消して
    /// `create_new` で新 lock を張る。これにより「`remove_file` → `create_new`」を **奪取者ごとに別ファイル経由**で
    /// 行うことになり、別の奪取者の `create_new`(O_EXCL) と「lock path の作成」だけが衝突する。lock path の作成は
    /// O_EXCL なのでちょうど 1 人だけ成功し、敗者は `None`（skip）へ倒れる。重要なのは **lock path を直接 remove
    /// しない**こと（rename で中継へ退避してから消す）で、「A が新 lock を張った直後に B の remove が A の lock を
    /// 消す」remove-clobber が原理的に起きない。
    ///
    /// rename 成功は「奪取権の獲得」ではなく「孤児退避の権利」であり、新 lock の真の取得点は最後の `create_new`
    /// （O_EXCL）に集約する。rename で奪った中身が判定〜rename 間に別勝者の fresh lock へ差し替わっていれば（TOCTOU）、
    /// 奪ったのは孤児でなく生きた lock なので [`restore_fresh_stolen_lock`] で **非破壊に元へ戻して** `None`（skip）へ
    /// 倒し、生きた owner を横取りしない。rename 失敗（src 消滅）は、別プロセスが既に退避・張替え中なので奪取せず
    /// `None`（skip）へ倒す（無更新は安全側）。
    ///
    /// なお本関数は **奪取区間（[`STEAL_SECTION_MUTEX`] でプロセス内 1 スレッドへ直列化）内**でのみ呼ばれるため、
    /// 同一プロセスの別スレッドが「現世代の fresh lock を退避して `lock_path` を空ける」cross-generation rename は
    /// 起きない（その退避が同時保持 2 の主因だった）。プロセス間は file `O_EXCL` が担う。`libc` を直呼びせず std の
    /// rename / create_new のみで実現する。
    fn steal_stale_lock_file_via_rename(path: &Path) -> Result<Option<Self>> {
        let stealing =
            path.with_file_name(format!("{LOCK_FILE}.stealing.{}", steal_rename_suffix()));
        match fs::rename(path, &stealing) {
            Ok(()) => {}
            // src 不在: 別プロセスが先に lock を退避中（rename 退避 → create_new の窓）か解放した。ここで
            // `create_new` を試みると、別勝者が原 lock を退避してまだ新 lock を張っていない窓に割り込んで二重奪取
            // しうる（path が一時的に空になるため）。よって奪取せず skip する。真に解放済みの空 lock は、次回
            // `try_acquire` 冒頭の `create_new_lock`（[`try_acquire`]）が正規に取得する（無更新は安全側）。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => {
                return Err(anyhow::Error::from(error)
                    .context(format!("failed to steal stale lock {}", path.display())));
            }
        }
        // 奪った中身を再確認する。判定〜rename 間に fresh lock へ差し替わっていれば横取りせず、生きた owner の
        // lock を **非破壊で元へ戻す**（[`restore_fresh_stolen_lock`]）。fresh 中継を `remove_file` で捨てると、その
        // owner の `UpdateLock` は依然生きているのに `lock_path` が空席へ戻り、後続の `create_new_lock`(O_EXCL) が
        // 2 本目の lock を取得して同時保持 2 になるため、復元は create_new(O_EXCL) で非破壊に行う。
        if Self::lock_is_fresh(&stealing) {
            Self::restore_fresh_stolen_lock(&stealing, path);
            return Ok(None);
        }
        // 真に孤児だった。中継を消して新 lock を O_EXCL で張る（lock path の作成は O_EXCL でちょうど 1 人が成功）。
        let _ = fs::remove_file(&stealing);
        Self::create_new_lock(path)
    }

    /// rename CAS で奪ったが fresh（生きた owner の実 lock）だった中継ファイルを、元の `lock_path` へ非破壊で戻す。
    ///
    /// **二重保持を作らないための復元規律（finding 3368585963）**: 奪った中継 `stealing` が生きた owner の lock
    /// だった場合、その owner の `UpdateLock` は依然生存している。中継を `remove_file` で捨てると `lock_path` が
    /// 空席へ戻り、後続の `create_new_lock`(O_EXCL) が 2 本目の lock を取得して同時保持が 2 になる。これを断つため、
    /// fresh 中継は **絶対に黙って捨てず**、その内容を `lock_path` へ復元する。
    ///
    /// **復元は `fs::rename` を使わない**: POSIX の `rename` は dst を**無条件に上書き**するため、別 owner が既に
    /// `lock_path` に張った新 lock を rename 復元が黙って潰し、その owner の `UpdateLock` を `lock_path` から消して
    /// しまう（別経路の二重保持窓）。よって復元は **`create_new`(O_EXCL) で `lock_path` を埋める**非破壊操作にする:
    /// `lock_path` が空なら奪った内容をそのまま書き戻し（owner の lock が復帰）、既に別 owner の lock が在れば
    /// `AlreadyExists` で書き込まず中継だけ捨てる（生きた lock は `lock_path` に 1 本在るので空席化しない）。
    /// `create_new` は同時実行を O_EXCL で 1 人に直列化するので、復元と別 owner の取得が競合しても上書き衝突しない。
    /// 書込み失敗（I/O 異常）など `lock_path` を埋められなかった場合は中継を**消さずに残し**（消すと空席化＝二重保持
    /// 窓を開く）、`update.lock.stealing.*` 中継として次回サイクルの孤児掃除に委ねる。`libc` を直呼びせず std の
    /// read / create_new / remove のみで実現する。
    fn restore_fresh_stolen_lock(stealing: &Path, path: &Path) {
        // 奪った fresh lock の内容を読み、`lock_path` が空なら同内容で原子的に復元する（O_EXCL なので上書きしない）。
        let Ok(content) = fs::read(stealing) else {
            // 中身を読めない（既に別経路で消えた等）。空席化を避けるため中継はそのまま残す。
            return;
        };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                if file.write_all(&content).is_ok() {
                    // owner の lock が `lock_path` へ復帰した。中継は不要なので捨てる。
                    let _ = fs::remove_file(stealing);
                }
                // 書込み失敗時は `lock_path` が空内容で残るが、中継は残して二重保持窓を作らない（次回掃除に委ねる）。
            }
            // 別 owner の lock が既に `lock_path` を埋めている。生きた lock は 1 本だけなので中継は捨ててよい。
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(stealing);
            }
            // それ以外の失敗（I/O 異常）。中継を消すと空席化＝二重保持窓を開くため、消さず残す。
            Err(_) => {}
        }
    }

    /// `create_new`（`O_CREAT|O_EXCL`）で lock を新規作成する。成功で `Some`、既存（`AlreadyExists`）で `None`。
    ///
    /// 作成時は診断・staleness 判定（pid 生存 + 開始時刻 identity + timestamp）用に
    /// `pid\nepoch_secs\nstart_token`（[`current_lock_payload`]）を書く。書込み失敗は致命にしない
    /// （その場合 staleness 判定は保守的に「生存中」へ倒れ、誤奪取を避ける）。
    fn create_new_lock(path: &Path) -> Result<Option<Self>> {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                let _ = write!(file, "{}", current_lock_payload());
                Ok(Some(Self {
                    path: Some(path.to_path_buf()),
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(anyhow::Error::from(error)
                .context(format!("failed to acquire lock {}", path.display()))),
        }
    }

    /// 既存 lock ファイルが stale（孤児）かを pid 生存 + timestamp で判定する。
    ///
    /// payload の pid が生存中なら（[`is_stale_lock_owner`]）timestamp が [`LOCK_STALE_SECS`] を超えていても
    /// 非 stale（奪取しない）に倒す。6h を超える長時間の初回適用・大 rebuild がまだ走っている live owner の
    /// lock を後続が奪取して二重適用する退行を防ぐ。pid が消えた孤児（kill / 再起動）か pid 解析不能のときだけ
    /// timestamp 規則で stale を判定する。読取り失敗・lock 消滅は保守的に「stale ではない」へ倒す。
    fn existing_lock_is_stale(path: &Path) -> bool {
        let Ok(content) = fs::read_to_string(path) else {
            return false;
        };
        is_stale_lock_owner(&content, now_epoch_secs(), LOCK_STALE_SECS)
    }
}

/// lock ファイルの内容（`pid\nepoch_secs\nstart_token`）を組み立てる純粋関数。
///
/// 1 行目は owner の pid、2 行目は staleness 判定に使う取得時刻（UNIX epoch 秒）、3 行目は **owner プロセスの
/// 固有 identity**（プロセス開始時刻トークン。空なら省略相当の空行）。
///
/// **3 行目（start token）を持つ理由（finding 3376248521 — pid 再利用の誤判定回避）**: 旧 payload は `pid` 生存
/// だけで owner 実行中とみなしていたため、`dotfiles update` が kill/OOM/再起動で `Drop` されず lock を残した後に
/// OS が **同じ pid を無関係な長寿命プロセスへ再利用**すると、その別プロセスが生存している限り `kill -0` が成功して
/// timestamp が 6h を超えても lock が永久に非 stale 扱いになり、auto-update が手動削除まで復旧しない。pid に加えて
/// owner プロセスの開始時刻トークンを控え、判定時に「pid 生存 **かつ** 開始時刻が一致」のときだけ live owner とみなす
/// ことで、pid 再利用（別プロセス＝開始時刻が異なる）を孤児として正しく回収できるようにする。`start_token` が空
/// （取得不能環境）の場合は 3 行目を空行にし、判定側は pid 生存のみの後方互換へ保守的に倒れる。
fn lock_payload(pid: u32, epoch_secs: u64, start_token: &str) -> String {
    format!("{pid}\n{epoch_secs}\n{start_token}\n")
}

/// 現在のプロセスを owner とする lock/steal marker payload を組み立てる。
///
/// pid・取得時刻・**現在プロセスの開始時刻トークン**（[`process_start_token`]）を [`lock_payload`] で連結する。
/// 開始時刻が `ps` から取得できない環境では空トークン（3 行目空行）で書き、判定側は pid 生存のみの後方互換へ
/// 保守的に倒れる（取得不能でも lock 取得自体は止めない）。
fn current_lock_payload() -> String {
    let pid = std::process::id();
    let start_token = process_start_token(pid).unwrap_or_default();
    lock_payload(pid, now_epoch_secs(), &start_token)
}

/// lock 内容（`pid\nepoch_secs\nstart_token`）の payload 2 行目（epoch 行）と現在時刻から staleness を判定する純粋関数。
///
/// 2 行目を epoch 秒として解析し、`now - acquired >= threshold` なら stale（孤児）とみなす。timestamp 行が
/// 無い / 解析不能 / 未来時刻（負の経過）は保守的に「stale ではない」へ倒し、生存中の適用を横取りしない。
/// 純粋関数として時刻・閾値を引数化し、奪取条件を I/O 無しで単体検証できるようにする。
///
/// **pid 生存は別判定で前置する**（本関数は timestamp だけを見る）。lock owner が `LOCK_STALE_SECS`（6h）を
/// 超える長時間の初回適用や大 rebuild をまだ実行中でも、本関数だけで判定すると timestamp 超過で stale 扱いに
/// なり後続が lock を奪取して二重適用しうる。よって呼び出し側（[`is_stale_lock_owner`]）は **payload の pid が
/// 生存中なら timestamp に依らず非 stale** とし、live owner の lock を奪取しない。本関数は pid 解析不能・
/// timestamp 単独判定のための純粋部品として残し、pid 生存確認は I/O を伴うため分離する。
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

/// lock/steal marker payload（`pid\nepoch_secs\nstart_token`）の owner が孤児（奪取可）かを
/// **pid 生存 + プロセス開始時刻 identity + timestamp** で判定する。
///
/// **判定順**: payload の 1 行目 pid を解析し、その pid が生存していて（[`pid_is_alive`]）、かつ
/// **payload が控えた owner プロセスの開始時刻トークンが現在その pid が持つ開始時刻と一致**するなら、owner は
/// 実行中とみなし timestamp が `threshold_secs` を超えていても **非 stale（奪取しない）** を返す。これにより、初回
/// 適用や大 rebuild が `LOCK_STALE_SECS`（6h）を超えてまだ走っている live owner の lock を後続が横取りし、
/// 2 本目の `dotfiles update` を同時進行させて switch / marker 更新の排他を壊す退行を防ぐ。
///
/// **pid 再利用の孤児を回収する（finding 3376248521）**: owner が kill/OOM/再起動で消えた後に OS が同じ pid を
/// 無関係な長寿命プロセスへ再利用すると、pid 生存だけでは live owner と誤判定し、timestamp が 6h を超えても lock が
/// 永久に非 stale になって auto-update が手動削除まで復旧しない。payload の開始時刻トークンと **現在その pid が持つ
/// 開始時刻**を照合し、不一致（= 別プロセスが pid を再利用した）なら owner は既に消えた孤児とみなして timestamp 規則
/// （[`is_stale_lock`]）へ委ね、6h 超過なら回収する。
///
/// **取得不能/旧 payload は保守的に倒す**: payload が start token を持たない（3 行目が空 = 旧 payload・取得不能環境で
/// 控えられなかった）場合、または現在の開始時刻を `ps` から取得できない場合は、identity 照合を強制できないため
/// **pid 生存のみの後方互換判定**（pid 生存なら非 stale）へ保守的に倒し、live owner を誤奪取しない。pid 不一致による
/// 誤回収より、live owner 横取りによる二重適用の方が危険なため、不確実時は「奪取しない」へ寄せる。pid 行が無い /
/// 解析不能なら timestamp だけで判定する（最古の旧 payload 互換）。`kill -0` / `ps` は std の `Command` 経由で
/// 呼び、`libc` を直呼びしない。
fn is_stale_lock_owner(content: &str, now_secs: u64, threshold_secs: u64) -> bool {
    if let Some(pid) = content
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<u32>().ok())
        && pid_is_alive(pid)
        && lock_owner_identity_matches(content, pid)
    {
        // owner が生存中かつ識別子一致（pid 再利用でない）。timestamp が古くても奪取しない（二重実行を防ぐ）。
        return false;
    }
    is_stale_lock(content, now_secs, threshold_secs)
}

/// payload が控えた owner の開始時刻トークンが、現在その pid が持つ開始時刻と一致するかを判定する純粋寄り関数。
///
/// payload の 3 行目（[`lock_payload`] が書く start token）を取り出し、`pid` の現在の開始時刻
/// （[`process_start_token`]）と照合する。**両方が得られて一致したときだけ `true`（同一 owner）**、それ以外は
/// 保守的に `true`（識別不能なら live owner 扱い＝奪取しない側へ倒す）を返す:
/// - payload に start token が無い（旧 payload・取得不能で空行）→ 照合できないので `true`（pid 生存判定のみへ縮退）。
/// - 現在の開始時刻を取得できない（`ps` 失敗・対象 pid 消滅直後など）→ `true`（誤奪取を避ける保守側）。
/// - 両方あって **不一致** → `false`（pid 再利用＝別プロセス。owner は孤児として回収可）。
///
/// pid 再利用の誤回収より live owner 横取りの方が危険なため、識別不能（token 欠落・取得不能）は「奪取しない」へ
/// 寄せる。不一致が確定したときだけ孤児として倒す。`ps` 実行は [`process_start_token`] に閉じる。
fn lock_owner_identity_matches(content: &str, pid: u32) -> bool {
    let stored_token = content
        .lines()
        .nth(2)
        .map(str::trim)
        .filter(|token| !token.is_empty());
    let Some(stored) = stored_token else {
        // payload に start token が無い（旧 payload・取得不能で空行）。pid 生存判定のみへ後方互換縮退する。
        return true;
    };
    match process_start_token(pid) {
        // 現在の開始時刻が得られた: 一致なら同一 owner、不一致なら pid 再利用（別プロセス）で孤児。
        Some(current) => current == stored,
        // 現在の開始時刻を取得できない（`ps` 失敗）。識別不能なので誤奪取を避けて live owner 扱いに倒す。
        None => true,
    }
}

/// 指定 pid の **プロセス開始時刻トークン**（owner identity）を `ps -o lstart= -p <pid>` から取得する。
///
/// プロセス開始時刻は pid が生きている間は不変で、pid が再利用（別プロセスへ割り当て）されると変わるため、
/// 「同じ pid が同じ owner か」を識別する固有値として使える（finding 3376248521 の pid 再利用検知）。`ps` の
/// `lstart` 列（プロセス開始の壁時計時刻）を取り、ロケール依存の前後空白・連続空白を 1 個へ正規化して安定化する
/// （payload 書込み時と判定時で同一正規化を通すため、表記揺れで誤不一致にならない）。空出力・`ps` 起動失敗・非 0
/// 終了・対象 pid 不在は `None`（取得不能）で、呼び出し側は保守的に live owner 扱いへ倒す。`std::process::Command`
/// で `ps` を起動し、`libc` を直接呼ばない（リポジトリ規約: libc 直呼び禁止）。
fn process_start_token(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .arg("-o")
        .arg("lstart=")
        .arg("-p")
        .arg(pid.to_string())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let token = normalize_start_token(&text);
    if token.is_empty() { None } else { Some(token) }
}

/// `ps -o lstart=` 出力を、表記揺れに依らない安定トークンへ正規化する純粋関数。
///
/// 前後空白を除き、連続空白（ロケール依存の桁揃え空白を含む）を単一空白へ畳む。payload 書込み時と staleness 判定時で
/// 同一正規化を通すことで、同じプロセス開始時刻が空白表記の差で誤不一致になるのを防ぐ。正規化を I/O から切り離し、
/// 空白畳み込み規則を単体検証できるようにする。
fn normalize_start_token(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// pid が生存しているかを `kill -0 <pid>` の終了コードで判定する（`libc` 直呼び禁止のため外部コマンド経由）。
///
/// `kill -0` はシグナルを送らず、対象 pid へシグナルを送れる（= プロセスが存在し権限がある）かだけを確認する
/// POSIX 慣用である。終了コード 0（成功）なら生存とみなす。pid が存在しない場合 `kill` は非 0 で失敗するので
/// 非生存（孤児）と判定する。`kill` の spawn 自体に失敗した場合は **保守的に「生存」へ倒す**（誤って live
/// owner の lock を奪取しないため）。`std::process::Command` で `kill` を起動し、`libc::kill` を直接呼ばない。
fn pid_is_alive(pid: u32) -> bool {
    match std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        // exit 0 = シグナル送出可能 = 生存。非 0 = pid 不在（孤児）。
        Ok(status) => status.success(),
        // kill を起動できない異常時は保守的に「生存」へ倒し、live owner を誤奪取しない。
        Err(_) => true,
    }
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
    /// defer→commit を 1 つの darwin 実行サイクルへ固定する **サイクル token**（ラッパーが生成し home/commit へ
    /// 同値を渡す）。
    ///
    /// `--defer-rev-marker` と併用すると、defer 時に `deferred-rev` と同じ瞬間にこの token を `deferred-token` へ
    /// 控える。`--commit-rev-marker` と併用すると、commit は `deferred-token` がこの token と一致する時だけ
    /// `deferred-rev` を確定する。darwin 実行中（user lock 解放後）に別サイクルが `deferred-rev` を上書きしても、
    /// token 不一致を検知して **root が適用していない後続サイクルの pin を確定しない**（finding 3368519975）。
    /// 未指定なら従来どおり（token 検証無し・現在 pin への後方互換縮退）。
    #[arg(long, value_name = "TOKEN")]
    rev_marker_token: Option<String>,
}

#[cfg(test)]
mod tests {
    //! auto 経路の引数列・pin 解析・state dir 解決・last-applied-rev 原子書込み・lock 競合 skip を固定する。

    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use std::ffi::OsString as TestOsString;

    use super::{
        CommitDecision, CommitWriteback, DEFERRED_NIXPKGS_REV, DEFERRED_REV, DEFERRED_TOKEN,
        LAST_APPLIED_HOME_REV, LAST_APPLIED_LOCK_ID, LAST_APPLIED_NIXPKGS_REV, LAST_APPLIED_REV,
        LAST_RUN_LOG, LAST_SUMMARIZED_AT, LAST_SUMMARIZED_HOME_AT, LOCK_FILE, LOCK_STALE_SECS,
        PENDING_SUMMARY, STEAL_MARKER_STALE_SECS, SummaryScope, UpdateLock, UpdateOptions,
        append_pending_summary, clear_deferred_markers, commit_apply_markers,
        commit_writeback_plan, copy_history_dir, is_stale_lock, is_stale_lock_owner,
        lock_content_id, lock_payload, parse_input_source_path, parse_nixpkgs_rev, parse_repo_pin,
        pid_is_alive, present_and_commit_summary, present_summary, read_deferred_nixpkgs_rev,
        read_deferred_rev, read_deferred_token, read_last_applied_home_lock_id,
        read_last_applied_home_rev, read_last_applied_lock_id, read_last_applied_rev,
        read_last_summarized_at, read_last_summarized_home_at, replace_history_dir_atomically,
        resolve_committed_marker, resolve_state_dir, should_switch, should_switch_full,
        should_switch_home, should_switch_home_full, sync_history_from_source, update_args,
        write_deferred_nixpkgs_rev, write_deferred_rev, write_deferred_token,
        write_last_applied_home_lock_id, write_last_applied_home_rev, write_last_applied_lock_id,
        write_last_applied_nixpkgs_rev, write_last_applied_rev, write_last_summarized_at,
        write_last_summarized_home_at,
    };
    use crate::update_history::domain::wire::PackageSourceFilter;
    use anyhow::anyhow;
    use clap::Parser;

    /// `UpdateOptions` を clap 経由で解析するためのテスト専用ラッパー。
    ///
    /// `UpdateOptions` のフィールドは private で直接構築できないため、`commit_apply_markers` /
    /// `run` 相当の経路を target 別に検証するには clap 解析で組み立てる。先頭にダミー実行名を補う。
    #[derive(Parser)]
    struct TestUpdateCli {
        #[command(flatten)]
        update: UpdateOptions,
    }

    /// 引数列から `UpdateOptions` を解析する（先頭にダミー実行名を補う）。失敗は文脈付き `Err`。
    fn parse_update(args: &[&str]) -> crate::Result<UpdateOptions> {
        let mut argv = vec!["dotfiles"];
        argv.extend_from_slice(args);
        TestUpdateCli::try_parse_from(argv)
            .map(|cli| cli.update)
            .map_err(|error| anyhow!("parse update options: {error}"))
    }

    /// 引数列を比較しやすいよう `OsString` を文字列へ揃える。
    fn as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    /// テスト専用の一時ディレクトリ（TMPDIR 配下）を作る。失敗は `crate::Result` で伝播する
    /// （リポジトリ Rust スタイル: テストを含め unwrap/expect を使わない）。
    fn temp_dir(tag: &str) -> crate::Result<PathBuf> {
        let mut dir = std::env::temp_dir();
        dir.push(format!("dotfiles-update-{}-{tag}", std::process::id()));
        tmkdirp(&dir)?;
        Ok(dir)
    }

    /// テスト用 fs ヘルパ（unwrap/expect を避け、`?` 伝播へ寄せるための薄いラッパ）。失敗は文脈付き
    /// `crate::Result` の `Err` にする。テストの set up / 観測で panic させず Result で扱うための支援境界。
    /// `Path`/`PathBuf` のいずれも受けられるよう `impl AsRef<Path>` で取る。
    fn tmkdirp(path: impl AsRef<Path>) -> crate::Result<()> {
        let path = path.as_ref();
        std::fs::create_dir_all(path)
            .map_err(|error| anyhow!("create dir {}: {error}", path.display()))
    }

    /// テスト用 fs ヘルパ: ファイル書込み。失敗は文脈付き `Err`。
    fn twrite(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> crate::Result<()> {
        let path = path.as_ref();
        std::fs::write(path, contents).map_err(|error| anyhow!("write {}: {error}", path.display()))
    }

    /// テスト用 fs ヘルパ: ファイル読取り（文字列）。失敗は文脈付き `Err`。
    fn tread(path: impl AsRef<Path>) -> crate::Result<String> {
        let path = path.as_ref();
        std::fs::read_to_string(path).map_err(|error| anyhow!("read {}: {error}", path.display()))
    }

    /// テスト用 fs ヘルパ: ファイル削除。失敗は文脈付き `Err`。
    fn tremove_file(path: impl AsRef<Path>) -> crate::Result<()> {
        let path = path.as_ref();
        std::fs::remove_file(path).map_err(|error| anyhow!("remove {}: {error}", path.display()))
    }

    /// テスト用 fs ヘルパ: rename。失敗は文脈付き `Err`。
    fn trename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> crate::Result<()> {
        let (from, to) = (from.as_ref(), to.as_ref());
        std::fs::rename(from, to)
            .map_err(|error| anyhow!("rename {} -> {}: {error}", from.display(), to.display()))
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
    fn home_only_dedup_uses_home_scope_marker_without_starving_darwin() {
        // 退行 finding 3374863446 固定（pin 注入のユニットテスト）: home-only catch-up（`update home`・非 defer）の
        // 適用要否は home スコープ marker で判定し、毎ログイン同一 pin 再適用の無限ループを止める。同時に、home
        // スコープしか確定しないため全体適用（darwin を含む）の `should_switch` 判定を動かさず darwin を starve
        // させないことを固定する。
        //
        // home marker が current_pin と一致すれば skip（前回 home-only 適用済み）。
        assert!(
            !should_switch_home(Some("pin-a"), None, "pin-a"),
            "home marker 一致なら home-only は再適用しない（無限ループ退行の是正）"
        );
        // home marker 不在（初回 home-only）でも、全体適用が確定した `last-applied-rev` が一致すれば home は適用済み
        // とみなし skip する（全体適用は home を含むため）。
        assert!(
            !should_switch_home(None, Some("pin-a"), "pin-a"),
            "全体適用が確定した pin と一致すれば home-only は再適用しない"
        );
        // どちらの marker も current_pin に一致しなければ適用する（新 pin への追随）。
        assert!(
            should_switch_home(Some("pin-old"), Some("pin-old"), "pin-new"),
            "home/全体いずれの marker も新 pin と異なれば home-only は switch する"
        );
        // 初回（両 marker 不在）は必ず適用する。
        assert!(
            should_switch_home(None, None, "first"),
            "両 marker 不在の初回は home-only も必ず switch する"
        );
        // **darwin 非 starve の根拠**: home-only が home marker だけ確定しても全体スコープの `last-applied-rev`
        // （full_rev）は home marker の値で動かない。全体適用の適用要否を見る `should_switch` は full_rev を読むため、
        // home marker のみが新 pin で確定された状態でも、全体適用は依然 switch 要（true）と判定される。
        assert!(
            should_switch(Some("pin-old"), "pin-new"),
            "home marker 確定は全体スコープ判定（should_switch）を動かさず darwin は依然適用要"
        );
    }

    #[test]
    fn home_only_apply_commits_only_home_scope_marker() -> crate::Result<()> {
        // 退行 finding 3374863446 固定（実行後 marker 確定）: home-only `update home`（非 defer）の適用後 marker 確定
        // （`commit_apply_markers`）が **home スコープ marker（`last-applied-home-rev`）だけ**を確定し、全体スコープ
        // marker（`last-applied-rev`/`last-applied-nixpkgs-rev`/`last-applied-lock-id`）を一切動かさないことを固定する。
        // これにより次回 home-only は dedup でき、かつ darwin を含む全体適用は未適用 pin を依然 switch 要と判定する。
        let dir = temp_dir("home-marker")?;
        let _ = std::fs::remove_file(dir.join(LAST_APPLIED_HOME_REV));
        let _ = std::fs::remove_file(dir.join(LAST_APPLIED_REV));
        let _ = std::fs::remove_file(dir.join(LAST_APPLIED_NIXPKGS_REV));

        // `update home`（非 defer・非 full）を clap 解析で組み立て、適用後 marker 確定を実行する。config_dir は
        // home-only 非 full 経路では lock-id を読まないためダミーで足りる。
        let options = parse_update(&["home"])?;
        commit_apply_markers(
            &dir,
            &options,
            Path::new("/nonexistent-config"),
            "pin-home",
            "nixpkgs-home",
            true, // history_synced: 履歴同期成功時に home スコープ marker を確定する。
            false,
        )?;

        // home スコープ marker だけ確定する。
        assert_eq!(
            read_last_applied_home_rev(&dir)?,
            Some("pin-home".to_string()),
            "home-only 適用後は home スコープ marker を確定する"
        );
        // 全体スコープ marker は一切動かさない（darwin starve 回避）。
        assert_eq!(
            read_last_applied_rev(&dir)?,
            None,
            "home-only 適用は全体スコープの last-applied-rev を確定しない（darwin 非 starve）"
        );
        assert!(
            !dir.join(LAST_APPLIED_NIXPKGS_REV).exists(),
            "home-only 適用は全体スコープの last-applied-nixpkgs-rev を書かない"
        );

        // 次回 home-only の適用要否: 同一 pin は home marker 一致で skip、新 pin は switch。
        let home_rev = read_last_applied_home_rev(&dir)?;
        let full_rev = read_last_applied_rev(&dir)?;
        assert!(
            !should_switch_home(home_rev.as_deref(), full_rev.as_deref(), "pin-home"),
            "確定後の同一 pin は home-only で skip する（無限ループ退行の是正）"
        );
        assert!(
            should_switch_home(home_rev.as_deref(), full_rev.as_deref(), "pin-next"),
            "新 pin は home-only で switch する（追随）"
        );
        // darwin 非 starve: 全体適用が読む `last-applied-rev` は None のままなので、全体適用は pin-home を依然
        // switch 要（適用要）と判定する。
        assert!(
            should_switch(full_rev.as_deref(), "pin-home"),
            "home-only 適用後も全体適用（darwin 含む）は同 pin を依然適用要と判定する"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn apply_dedup_markers_not_committed_when_history_sync_failed() -> crate::Result<()> {
        // finding 3376248509 退行固定: 履歴同期未成功（`history_synced == false`）のとき、非 defer の apply-dedup
        // marker（全体 / home スコープ）を確定しない。確定すると次回同一 pin で sync より前に早期 return し、通信
        // 復旧後も同じ pin で再同期・再要約できず、次の bump まで show/pending が空/古いままになる。確定しないことで
        // 次回同一 pin で switch（冪等）→ 再同期 → 再要約を試せる。
        let dir = temp_dir("history-sync-fail-marker")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // 全体適用（target 省略 = all・非 defer・非 full）。history_synced=false では last-applied-* を書かない。
        let all = parse_update(&[])?;
        commit_apply_markers(
            &dir,
            &all,
            Path::new("/nonexistent-config"),
            "pin-all",
            "nixpkgs-all",
            false, // history_synced=false
            false,
        )?;
        assert_eq!(
            read_last_applied_rev(&dir)?,
            None,
            "履歴同期失敗時は全体 last-applied-rev を確定しない（次回同一 pin で再同期できるよう）"
        );
        assert!(
            !dir.join(LAST_APPLIED_NIXPKGS_REV).exists(),
            "履歴同期失敗時は last-applied-nixpkgs-rev を書かない"
        );

        // home 部分適用（非 defer）。history_synced=false では home スコープ marker も書かない。
        let home = parse_update(&["home"])?;
        commit_apply_markers(
            &dir,
            &home,
            Path::new("/nonexistent-config"),
            "pin-home",
            "nixpkgs-home",
            false, // history_synced=false
            false,
        )?;
        assert_eq!(
            read_last_applied_home_rev(&dir)?,
            None,
            "履歴同期失敗時は home スコープ marker も確定しない"
        );

        // 対照: history_synced=true なら全体適用で last-applied-rev を確定する（既存挙動を保つ）。
        commit_apply_markers(
            &dir,
            &all,
            Path::new("/nonexistent-config"),
            "pin-all",
            "nixpkgs-all",
            true, // history_synced=true
            false,
        )?;
        assert_eq!(
            read_last_applied_rev(&dir)?,
            Some("pin-all".to_string()),
            "履歴同期成功時は従来どおり last-applied-rev を確定する"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn home_full_apply_commits_home_scope_lock_id_marker() -> crate::Result<()> {
        // finding 3376248543 固定（実行後 marker 確定）: `update home --full`（非 defer）の適用後 marker 確定が
        // home スコープ lock-id marker（`last-applied-home-lock-id`）を確定し、全体スコープの `last-applied-lock-id`
        // を動かさないことを固定する（全体 `--full` の dedup を壊さず darwin starve を回避）。
        let dir = temp_dir("home-full-lockid")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // config_dir に flake.lock を置き、read_lock_id が決定論的 identity を返せるようにする。
        let config_dir = dir.join("config");
        tmkdirp(&config_dir)?;
        twrite(config_dir.join("flake.lock"), b"{\"nodes\":{}}\n")?;

        let options = parse_update(&["home", "--full"])?;
        commit_apply_markers(
            &dir,
            &options,
            &config_dir,
            "pin-home-full",
            "nixpkgs-home-full",
            true,
            false,
        )?;

        // home スコープ pin marker と home スコープ lock-id marker を確定する。
        assert_eq!(
            read_last_applied_home_rev(&dir)?,
            Some("pin-home-full".to_string()),
            "home --full は home スコープ pin marker を確定する"
        );
        let expected_lock_id = super::lock_content_id(b"{\"nodes\":{}}\n");
        assert_eq!(
            read_last_applied_home_lock_id(&dir)?,
            Some(expected_lock_id),
            "home --full は home スコープ lock-id marker を確定する"
        );
        // 全体スコープの lock-id / pin marker は一切動かさない（全体 --full の dedup・darwin starve を壊さない）。
        assert_eq!(
            read_last_applied_lock_id(&dir)?,
            None,
            "home --full は全体スコープ last-applied-lock-id を確定しない"
        );
        assert_eq!(
            read_last_applied_rev(&dir)?,
            None,
            "home --full は全体スコープ last-applied-rev を確定しない"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn should_switch_home_full_uses_lock_identity_when_pin_unchanged() {
        // finding 3376248543 固定（判定純粋関数）: `update home --full` の switch 要否は home スコープ pin と
        // lock 全体 identity のいずれかの変化で決める。dotfiles pin 不変でも lock-id が変われば switch する。
        // pin も lock-id も前回 home `--full` 適用値と同一なら skip する。
        // pin 一致 + lock-id 一致 → skip。
        assert!(
            !should_switch_home_full(Some("pin"), None, "pin", Some("lock-a"), "lock-a"),
            "pin も lock-id も同一なら home --full は skip"
        );
        // pin 一致だが lock-id 変化（nixpkgs 等だけ動いた通常ケース）→ switch（旧 pin-only 判定の退行是正）。
        assert!(
            should_switch_home_full(Some("pin"), None, "pin", Some("lock-a"), "lock-b"),
            "pin 同一でも lock-id 変化なら home --full は switch する"
        );
        // pin 変化 → lock-id に依らず switch。
        assert!(
            should_switch_home_full(Some("pin-old"), None, "pin-new", Some("lock-a"), "lock-a"),
            "pin 変化なら home --full は switch する"
        );
        // home lock-id marker 不在（home --full 初回）→ lock 未適用とみなし switch。
        assert!(
            should_switch_home_full(Some("pin"), None, "pin", None, "lock-a"),
            "home lock-id marker 不在（初回）は home --full で switch する"
        );
        // 全体適用が確定した last-applied-rev（full_rev）が一致すれば pin 判定は skip 側だが、lock-id 不在なら
        // lock 変化扱いで switch（全体適用は全体 lock-id を別管理するため home --full は home lock-id で再判定する）。
        assert!(
            should_switch_home_full(None, Some("pin"), "pin", None, "lock-a"),
            "全体適用済み pin でも home lock-id 未確定なら home --full は lock 変化として switch"
        );
    }

    #[test]
    fn home_full_lock_id_marker_round_trips_and_respects_dry_run() -> crate::Result<()> {
        // home スコープ lock-id marker（`last-applied-home-lock-id`）の read/write 往復と dry-run 非書込を固定する。
        let dir = temp_dir("home-full-lockid-roundtrip")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        assert_eq!(read_last_applied_home_lock_id(&dir)?, None);

        write_last_applied_home_lock_id(&dir, "lock-id-x", false)?;
        assert_eq!(
            read_last_applied_home_lock_id(&dir)?,
            Some("lock-id-x".to_string())
        );

        // dry-run では書かない。
        let dir2 = temp_dir("home-full-lockid-dry")?;
        let _ = std::fs::remove_dir_all(&dir2);
        tmkdirp(&dir2)?;
        write_last_applied_home_lock_id(&dir2, "lock-id-y", true)?;
        assert_eq!(read_last_applied_home_lock_id(&dir2)?, None);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
        Ok(())
    }

    #[test]
    fn home_marker_round_trips_atomically_and_respects_dry_run() -> crate::Result<()> {
        // home スコープ marker（`last-applied-home-rev`）の read/write 往復と dry-run 非書込を固定する。
        let dir = temp_dir("home-rev")?;
        let _ = std::fs::remove_file(dir.join(LAST_APPLIED_HOME_REV));
        // 未書込みは None。
        assert_eq!(read_last_applied_home_rev(&dir)?, None);

        write_last_applied_home_rev(&dir, "home-pin", false)?;
        assert_eq!(
            read_last_applied_home_rev(&dir)?,
            Some("home-pin".to_string())
        );
        assert!(dir.join(LAST_APPLIED_HOME_REV).exists());
        // temp ファイルは rename 後に残らない。
        let leftover: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|error| anyhow!("read state dir: {error}"))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftover.is_empty());

        // dry-run は書き込まない。
        write_last_applied_home_rev(&dir, "home-pin-dryrun", true)?;
        assert_eq!(
            read_last_applied_home_rev(&dir)?,
            Some("home-pin".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
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
        let dir = temp_dir("rev")?;
        // 未書込みは None。
        assert_eq!(read_last_applied_rev(&dir)?, None);

        write_last_applied_rev(&dir, "rev-new", false)?;
        assert_eq!(read_last_applied_rev(&dir)?, Some("rev-new".to_string()));
        // temp ファイルは rename 後に残らない。
        let leftover: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|error| anyhow!("read state dir: {error}"))?
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
        let dir = temp_dir("lock")?;
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
        let dir = temp_dir("lock-dry")?;
        let lock = UpdateLock::try_acquire(&dir, true)?;
        assert!(lock.is_some());
        // dry-run は実ロックファイルを作らない。
        assert!(!dir.join(LOCK_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn is_stale_lock_uses_timestamp_threshold() {
        // P2-3: lock payload（`pid\nepoch_secs\nstart_token`）の 2 行目（epoch 行）を見て staleness を判定する純粋規則を固定。
        let now = 1_000_000u64;
        // 閾値以上古い → stale。
        assert!(is_stale_lock(
            &dead_lock_payload(42, now - LOCK_STALE_SECS),
            now,
            LOCK_STALE_SECS
        ));
        assert!(is_stale_lock(
            &dead_lock_payload(42, now - LOCK_STALE_SECS - 1),
            now,
            LOCK_STALE_SECS
        ));
        // 閾値未満（取得直後・実行中）→ 非 stale（横取りしない）。
        assert!(!is_stale_lock(
            &dead_lock_payload(42, now),
            now,
            LOCK_STALE_SECS
        ));
        assert!(!is_stale_lock(
            &dead_lock_payload(42, now - 1),
            now,
            LOCK_STALE_SECS
        ));
        // timestamp 行が無い / 解析不能 / 未来時刻は保守的に非 stale。
        assert!(!is_stale_lock("42\n", now, LOCK_STALE_SECS));
        assert!(!is_stale_lock("42\nnotnum\n", now, LOCK_STALE_SECS));
        assert!(!is_stale_lock(
            &dead_lock_payload(42, now + 100),
            now,
            LOCK_STALE_SECS
        ));
    }

    #[test]
    fn is_stale_lock_owner_skips_live_pid_even_when_timestamp_is_old() {
        // finding 3368559583 退行固定: lock owner の pid が生存中なら、timestamp が `LOCK_STALE_SECS`（6h）を
        // 超えていても stale 扱いしない（長時間の初回適用・大 rebuild を実行中の live owner を奪取しない）。
        let now = 2_000_000u64;
        let old = now - LOCK_STALE_SECS - 600; // 閾値を十分超過した古い取得時刻。

        // 自プロセス pid は確実に生存している。古い timestamp でも owner 生存（pid + 開始時刻一致）なら非 stale。
        let live_pid = std::process::id();
        assert!(pid_is_alive(live_pid), "current process must be alive");
        assert!(
            !is_stale_lock_owner(&live_lock_payload(old), now, LOCK_STALE_SECS),
            "live owner の lock は timestamp が古くても奪取しない"
        );
        // 対照: timestamp だけ見る is_stale_lock は同じ payload を stale と判定する（pid 生存で覆る前の判定）。
        assert!(
            is_stale_lock(&live_lock_payload(old), now, LOCK_STALE_SECS),
            "timestamp 単独では古いので stale（pid 生存ガードが効くことの対照）"
        );

        // pid が存在しない（プロセス kill / 再起動で消えた孤児）なら、6h 超過の timestamp で stale になり奪取可。
        let dead_pid = dead_pid_for_test();
        assert!(!pid_is_alive(dead_pid), "selected pid must be dead");
        assert!(
            is_stale_lock_owner(&dead_lock_payload(dead_pid, old), now, LOCK_STALE_SECS),
            "孤児 pid + 古い timestamp は stale（真の孤児は回収する）"
        );
        // 孤児 pid でも timestamp が新しければ非 stale（取得直後の owner を誤奪取しない）。
        assert!(
            !is_stale_lock_owner(&dead_lock_payload(dead_pid, now), now, LOCK_STALE_SECS),
            "孤児 pid でも timestamp が新しければ奪取しない"
        );
        // pid 行が無い旧 payload は timestamp だけで判定する（後方互換）。pid も timestamp も無ければ非 stale。
        assert!(
            !is_stale_lock_owner("\n", now, LOCK_STALE_SECS),
            "pid も timestamp も無い payload は保守的に非 stale"
        );
    }

    #[test]
    fn is_stale_lock_owner_reclaims_reused_pid_with_mismatched_start_token() {
        // finding 3376248521 退行固定: owner が落ちた後に OS が同じ pid を別プロセスへ再利用すると、pid 生存だけでは
        // live owner と誤判定して 6h 超でも非 stale のままになり、auto-update が手動削除まで復旧しない。payload の
        // 開始時刻トークンと現在 pid の開始時刻を照合し、不一致（= 別プロセスが pid を再利用）なら timestamp 規則で
        // 孤児として回収する。
        let now = 3_000_000u64;
        let old = now - LOCK_STALE_SECS - 600;

        // 自プロセス pid（確実に生存）だが、payload の開始時刻トークンは現在値と **異なる**（pid 再利用を模す）。
        let live_pid = std::process::id();
        assert!(pid_is_alive(live_pid), "current process must be alive");
        let real_token = super::process_start_token(live_pid);
        // 実トークンが取れる環境でのみ、不一致トークンで「pid 再利用 → 回収」を検証する。取れない環境（`ps`
        // 不在等）では identity 照合できず保守的に live owner 扱いになるため、その分岐は別 assert で固定する。
        if let Some(real) = real_token {
            let mismatched = format!("{real} REUSED-MARKER");
            assert_ne!(mismatched, real, "不一致トークンを確実に作る");
            let reused_payload = lock_payload(live_pid, old, &mismatched);
            assert!(
                is_stale_lock_owner(&reused_payload, now, LOCK_STALE_SECS),
                "pid 生存でも開始時刻トークン不一致（pid 再利用）+ 古い timestamp は stale（孤児回収）"
            );
            // 同じ不一致 payload でも timestamp が新しければ非 stale（取得直後を誤回収しない）。
            assert!(
                !is_stale_lock_owner(
                    &lock_payload(live_pid, now, &mismatched),
                    now,
                    LOCK_STALE_SECS
                ),
                "トークン不一致でも timestamp が新しければ回収しない"
            );
            // 対照: トークンが一致すれば（同一 owner）古い timestamp でも非 stale（奪取しない）。
            assert!(
                !is_stale_lock_owner(&lock_payload(live_pid, old, &real), now, LOCK_STALE_SECS),
                "開始時刻トークン一致 = 同一 owner は古くても奪取しない"
            );
        }

        // 取得不能/旧 payload（start token 空）は identity 照合できず pid 生存のみの後方互換へ倒れる: pid 生存なら
        // 古い timestamp でも非 stale（live owner 誤奪取を避ける保守側）。
        assert!(
            !is_stale_lock_owner(&dead_lock_payload(live_pid, old), now, LOCK_STALE_SECS),
            "start token 空（取得不能/旧 payload）は pid 生存のみで判定し live owner を奪取しない"
        );
    }

    #[test]
    fn normalize_start_token_collapses_whitespace() {
        // `ps -o lstart=` のロケール依存桁揃え空白・前後空白を畳んで安定トークンにする純粋規則を固定する。
        // 書込み時と判定時で同一正規化を通すため、空白表記差で同一プロセスが誤不一致にならないことを保証する。
        assert_eq!(
            super::normalize_start_token("  Mon Jun  9 05:53:09 2026  \n"),
            "Mon Jun 9 05:53:09 2026"
        );
        assert_eq!(super::normalize_start_token("   "), "");
        assert_eq!(super::normalize_start_token(""), "");
    }

    #[test]
    fn lock_owner_identity_matches_is_conservative_when_token_absent() {
        // payload に start token が無い（3 行目空・2 行 payload）場合は identity 照合できないため、保守的に `true`
        // （live owner 扱い = 奪取しない）へ倒す。pid 生存判定のみの後方互換を保証する。
        let live_pid = std::process::id();
        // 3 行目空（dead_lock_payload は token を空にする）。
        assert!(
            super::lock_owner_identity_matches(&dead_lock_payload(live_pid, 0), live_pid),
            "start token 空は identity 照合不能 → 保守的に一致扱い（奪取しない）"
        );
        // 2 行 payload（旧 payload。3 行目自体が無い）も保守的に一致扱い。
        assert!(
            super::lock_owner_identity_matches("123\n456\n", live_pid),
            "旧 2 行 payload は identity 照合不能 → 保守的に一致扱い"
        );
    }

    /// 自プロセス（生存中の live owner）の lock payload を、指定取得時刻 + **現在の開始時刻トークン**で組み立てる
    /// テスト helper。
    ///
    /// `is_stale_lock_owner` は pid 生存に加えて payload の開始時刻トークンが現在 pid の開始時刻と一致するときだけ
    /// live owner とみなす（finding 3376248521）。よって live owner を表す payload は自プロセスの実 start token を
    /// 控える必要があり、これを 1 箇所へ集約する。`ps` が取れない環境では空トークンへ縮退し、判定側は pid 生存のみの
    /// 後方互換で live owner 扱いになる。
    fn live_lock_payload(epoch: u64) -> String {
        let pid = std::process::id();
        let token = super::process_start_token(pid).unwrap_or_default();
        lock_payload(pid, epoch, &token)
    }

    /// 孤児（dead）pid 向けの lock payload を組み立てるテスト helper（start token は空）。
    ///
    /// dead pid では `pid_is_alive` が false になり identity 照合は短絡されるため、start token は空でよい。
    /// timestamp のみで stale 判定する純粋経路（`is_stale_lock`）の入力にも使える。
    fn dead_lock_payload(pid: u32, epoch: u64) -> String {
        lock_payload(pid, epoch, "")
    }

    /// テストで「確実に生存していない pid」を返す。子プロセスを spawn して即 wait し、回収済み pid を再利用
    /// する（その pid は wait 後に解放され、`kill -0` が必ず失敗する）。`libc` を使わず std のみで構成する。
    ///
    /// spawn/wait に失敗した場合は、ほぼ確実に割り当てられない大きな pid 値（PID_MAX 超過）へフォールバックして
    /// panic させない（unwrap/expect 不使用）。stale 判定は pid 不在で成立するため、未割当 pid でも目的を満たす。
    fn dead_pid_for_test() -> u32 {
        const UNLIKELY_LIVE_PID: u32 = 0x7fff_fffe;
        match std::process::Command::new("true").spawn() {
            Ok(mut child) => {
                let pid = child.id();
                // 回収して pid を解放する（以後この pid は不在 = stale 判定が成立する）。失敗しても致命にしない。
                let _ = child.wait();
                pid
            }
            Err(_) => UNLIKELY_LIVE_PID,
        }
    }

    #[test]
    fn try_acquire_does_not_steal_lock_held_by_live_pid() -> crate::Result<()> {
        // finding 3368559583 退行固定（経路）: payload の pid が生存中の lock は、timestamp が 6h を超えて
        // 古くても try_acquire が奪取せず skip（None）する。switch / marker 更新の排他を live owner に保つ。
        let dir = temp_dir("live-pid-lock")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        let lock_path = dir.join(LOCK_FILE);

        // 古い timestamp（6h 超過）だが、owner pid は自プロセス（確実に生存中）かつ開始時刻トークン一致。
        let old_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 600);
        let live_payload = live_lock_payload(old_epoch);
        twrite(&lock_path, &live_payload)?;

        // live owner の lock は奪取しない（None = skip）。
        assert!(
            UpdateLock::try_acquire(&dir, false)?.is_none(),
            "live owner の lock は timestamp が古くても奪取しない"
        );
        // lock ファイルは触られず残る（奪取で書き換えられていない）。
        assert_eq!(
            tread(&lock_path)?,
            live_payload,
            "live owner の lock payload は奪取されず保たれる"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn try_acquire_steals_stale_lock_but_skips_live_lock() -> crate::Result<()> {
        // P2-3 退行固定: プロセス kill 等で Drop されず残った stale lock は奪取して実行に進む。
        // 生存中（新しい timestamp）の lock は奪取せず skip する。
        let dir = temp_dir("lock-stale")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        let lock_path = dir.join(LOCK_FILE);

        // 古い timestamp かつ owner pid が不在（kill / 再起動で消えた孤児）の lock を置く。pid 生存ガードを
        // 確実に通すため、回収済み（確実に dead な）pid を使う。
        let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 60);
        twrite(
            &lock_path,
            dead_lock_payload(dead_pid_for_test(), stale_epoch),
        )?;
        // 奪取して取得成功する。
        let acquired = UpdateLock::try_acquire(&dir, false)?;
        assert!(acquired.is_some(), "stale lock must be stolen");
        // 奪取後の lock は現在時刻で書き直され、生存中扱いになる（別プロセスは skip）。
        assert!(UpdateLock::try_acquire(&dir, false)?.is_none());
        drop(acquired);

        // 解放後、新しい（生存中）lock を置くと奪取されない（live owner = 自プロセス + 開始時刻一致）。
        let fresh_epoch = super::now_epoch_secs();
        twrite(&lock_path, live_lock_payload(fresh_epoch))?;
        assert!(
            UpdateLock::try_acquire(&dir, false)?.is_none(),
            "live lock must not be stolen"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn last_applied_nixpkgs_rev_writes_independent_file_and_respects_dry_run() -> crate::Result<()>
    {
        // `last-applied-nixpkgs-rev` は適用成功時／commit 時に確定する state file で、適用済み nixpkgs rev を
        // 記録する（`--commit-rev-marker` 経路がこの値を確定する）。要約 span 起点は `at` カーソル
        // （`last-summarized-at`）へ移行したため、本 marker はもはや span 起点解決のフォールバックには使わない
        // （production では読まない write-only の記録）。ここでは書込みが `last-applied-rev` と別ファイルへ
        // 独立に行われ、dry-run では書かないことを固定する。
        let dir = temp_dir("applied-nixpkgs")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // 適用成功時に確定した nixpkgs rev を書く。dotfiles pin の `last-applied-rev` とは別ファイル。
        write_last_applied_nixpkgs_rev(&dir, "nixpkgs-applied-old", false)?;
        write_last_applied_rev(&dir, "dotfiles-pin", false)?;
        assert!(dir.join(LAST_APPLIED_NIXPKGS_REV).exists());
        assert_eq!(
            std::fs::read_to_string(dir.join(LAST_APPLIED_NIXPKGS_REV))?.trim(),
            "nixpkgs-applied-old"
        );
        assert_eq!(
            read_last_applied_rev(&dir)?,
            Some("dotfiles-pin".to_string())
        );

        // dry-run は書かない（既存の確定済み値を上書きしない）。
        write_last_applied_nixpkgs_rev(&dir, "should-not-write", true)?;
        assert_eq!(
            std::fs::read_to_string(dir.join(LAST_APPLIED_NIXPKGS_REV))?.trim(),
            "nixpkgs-applied-old"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 非 tty 適用 1 回ぶん（要約 → 要約済み marker 確定）を、本番 `run()` の defer 経路と同じ順序で実行する。
    ///
    /// span 起点は `last-summarized-at`（`at` カーソル。marker 無し = 初回 = `None` で全件）。要約「後」に
    /// `present_summary` が返した終端 `at` を marker へ進める（要約前に失敗すると marker が進まないことを
    /// 別テストで固定する）。`None`（空 span）のときは marker を進めない。
    fn apply_once_defer(state_dir: &Path) -> crate::Result<()> {
        let span_start_at = read_last_summarized_at(state_dir)?;
        // 非 tty 経路（background daemon の defer 適用）を決定論的に exercise するため stdout_is_terminal=false
        // を明示注入する。これで stdout が tty になる nix build sandbox でも pending-summary 追記経路を確実に通す。
        let summarized_at = present_summary(
            state_dir,
            span_start_at.as_deref(),
            PackageSourceFilter::All,
            false,
            false,
        )?;
        if let Some(at) = summarized_at {
            write_last_summarized_at(state_dir, &at, false)?;
        }
        Ok(())
    }

    #[test]
    fn same_day_defer_runs_append_update_block_only_once() -> crate::Result<()> {
        // 退行固定（A: show-once）: 同日に shell catch-up（defer）と daemon home（defer）が両方走る通常ケースで、
        // pending-summary に同一更新ブロック（N0->N1）が **1 回だけ** 入ることを決定論的に固定する。
        //
        // marker（`last-summarized-at`）を要約「後」に終端 `at` へ進めるため、2 回目の present_summary は起点 =
        // その `at` → `select_entries_after` がそれより後を選び空 span → 再追記しない。
        let dir = temp_dir("same-day-defer")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        // 履歴 chain: N0->N1（実 packages 1 件、at = 2026-06-01）。
        write_history(&dir, &[("N0", "N1")])?;
        // 1 回目（shell catch-up, defer）: marker 無し → 全件 → N0->N1 を追記し marker = at(0)。
        apply_once_defer(&dir)?;
        // 2 回目（daemon home, defer, 同日）: 起点 = marker(at(0))。新規無し → 空 span → 再追記しない。
        apply_once_defer(&dir)?;

        let pending = tread(dir.join(PENDING_SUMMARY))?;
        // 更新ブロック（宣言アプリ行 neovim-N0）は 1 回だけ現れる（二度見え＝退行を弾く）。
        let occurrences = pending.matches("neovim-N0").count();
        assert_eq!(
            occurrences, 1,
            "same-day defer runs must append the N0->N1 block exactly once: {pending}"
        );
        // marker は要約済み終端エントリの `at` に確定している（defer でも書く）。
        assert_eq!(read_last_summarized_at(&dir)?, Some(at_of(0)));

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn same_day_defer_does_not_redisplay_brew_only_n_to_n_updates() -> crate::Result<()> {
        // 退行固定（P2: brew-only 再表示抑止）: nixpkgs rev が動かない（`N -> N`）brew-only 更新が連続しても、
        // `at` カーソルで一度要約したら再表示しない。旧 nixpkgs-rev 起点は `N -> N` を越えられず、同じ
        // brew-only 更新を毎回再追記した。
        let dir = temp_dir("brew-only-n-to-n")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        // 2 夜とも nixpkgs rev が動かない（`nixpkgs_old == nixpkgs_new`）brew-only 更新。`at` だけが進む
        // （2026-06-01, 2026-06-02）。パッケージ名は別（neovim-Na / neovim-Nb）にして集約で潰れないようにする。
        write_history(&dir, &[("Na", "Na"), ("Nb", "Nb")])?;
        // 1 回目: marker 無し → 両エントリを要約し marker = 終端 at(1)。
        apply_once_defer(&dir)?;
        let pending = tread(dir.join(PENDING_SUMMARY))?;
        assert!(pending.contains("neovim-Na"), "初回は Na を要約: {pending}");
        assert!(pending.contains("neovim-Nb"), "初回は Nb を要約: {pending}");
        assert_eq!(read_last_summarized_at(&dir)?, Some(at_of(1)));
        let _ = std::fs::remove_file(dir.join(PENDING_SUMMARY)); // 消費済みとみなす。

        // 2 回目: 新規 brew-only 更新なし → marker(at(1)) 以降は空 → 再追記しない。
        apply_once_defer(&dir)?;
        let pending2 = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).unwrap_or_default();
        assert!(
            !pending2.contains("neovim-Na") && !pending2.contains("neovim-Nb"),
            "要約済み brew-only 更新を再表示しない: {pending2:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn partial_failure_before_summary_preserves_span() -> crate::Result<()> {
        // 退行固定（B: partial-failure 堅牢性）: switch/darwin が **要約前** に失敗した再実行で、要約 span が
        // 消えないことを固定する。要約「後」に marker を進める設計のため、要約前に失敗すれば marker は前回
        // 要約済み `at` のまま保たれ、再実行で未表示範囲を再び示せる。
        let dir = temp_dir("partial-failure")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        // 履歴 chain: N0->N1（at(0)）・N1->N2（at(1)）。N0->N1 は前回適用で要約済み、N1->N2 が未表示範囲。
        write_history(&dir, &[("N0", "N1"), ("N1", "N2")])?;
        // 前回の成功適用で **N0->N1 だけ**要約済み（marker = at(0)）にしておく（at(1) より前で止める）。
        let first = present_summary(&dir, None, PackageSourceFilter::All, false, false)?;
        // 全件選択されるので終端は at(1) になってしまう。partial-failure を模すため marker は at(0) に固定する。
        let _ = first;
        write_last_summarized_at(&dir, &at_of(0), false)?;
        let _ = std::fs::remove_file(dir.join(PENDING_SUMMARY)); // 前回ぶんは消費済みとみなす。

        // 今回: switch が要約「前」に失敗 → present_summary も marker 書込みも走らない。marker は at(0) のまま。
        let span_start = read_last_summarized_at(&dir)?;
        assert_eq!(
            span_start.as_deref(),
            Some(at_of(0).as_str()),
            "span start must be preserved at last-summarized at, not advanced"
        );

        // 再実行（switch 成功）で N1->N2（at(1)）を要約できる（未表示範囲が消えていない）。
        apply_once_defer(&dir)?;
        let pending = tread(dir.join(PENDING_SUMMARY))?;
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
    fn summarized_at_marker_round_trips_and_respects_dry_run() -> crate::Result<()> {
        // `last-summarized-at` marker の read/write 往復と dry-run 非書込を固定する。
        let dir = temp_dir("summarized-at-marker")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // marker 無し（初回）は None（全件 = 初回 catch-up）。
        assert_eq!(read_last_summarized_at(&dir)?, None);
        // 書込→読出で往復する。
        write_last_summarized_at(&dir, "2026-06-02T00:00:00Z", false)?;
        assert_eq!(
            read_last_summarized_at(&dir)?,
            Some("2026-06-02T00:00:00Z".to_string())
        );
        assert!(dir.join(LAST_SUMMARIZED_AT).exists());
        // dry-run は書き込まない（既存値を保つ）。
        write_last_summarized_at(&dir, "should-not-write", true)?;
        assert_eq!(
            read_last_summarized_at(&dir)?,
            Some("2026-06-02T00:00:00Z".to_string())
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
    fn write_history(state_dir: &Path, chain: &[(&str, &str)]) -> crate::Result<()> {
        let history_dir = state_dir.join(super::HISTORY_LOCAL_SUBDIR);
        tmkdirp(&history_dir)?;
        let mut toml = String::new();
        // 各エントリへ**記録順に増える一意な `at`** を与える（`at` カーソルの単調性の前提）。RFC3339 文字列の
        // 辞書順が時系列順に一致するよう日付を 1 日ずつ進める（`2026-06-01`, `2026-06-02`, ...）。
        for (index, (old, new)) in chain.iter().enumerate() {
            let day = index + 1;
            toml.push_str(&format!(
                "[[update]]\n\
                 at = \"2026-06-{day:02}T00:00:00Z\"\n\
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
        twrite(history_dir.join("2026-06.toml"), toml)?;
        Ok(())
    }

    /// chain index（0 始まり）に対応するエントリの `at`（[`write_history`] と同じ採番）。`at` カーソルの
    /// 期待値をテストで参照するためのヘルパ。
    fn at_of(index: usize) -> String {
        format!("2026-06-{:02}T00:00:00Z", index + 1)
    }

    /// nix package（`neovim`）と brew cask（`firefox`）の両方を 1 エントリに含む履歴を書く（finding 5 検証用）。
    ///
    /// `at = 2026-06-01`、nixpkgs rev は `N0->N1`。home-only 適用（`NixOnly`）は neovim だけを、darwin/全体適用
    /// （`All`）は neovim + firefox（cask）を要約に含めるべきことを exercise するための fixture。
    fn write_history_with_nix_and_brew(state_dir: &Path) -> crate::Result<()> {
        let history_dir = state_dir.join(super::HISTORY_LOCAL_SUBDIR);
        tmkdirp(&history_dir)?;
        let toml = "[[update]]\n\
             at = \"2026-06-01T00:00:00Z\"\n\
             nixpkgs_old = \"N0\"\n\
             nixpkgs_new = \"N1\"\n\
             reference = \"darwinConfigurations.ci\"\n\
             severity = \"minor\"\n\
             overall = \"2アプリ更新\"\n\
             \n\
             [[update.package]]\n\
             name = \"neovim\"\n\
             source = \"nix\"\n\
             old = \"1.0\"\n\
             new = \"1.1\"\n\
             change = \"upgraded\"\n\
             declared = true\n\
             \n\
             [[update.package]]\n\
             name = \"firefox\"\n\
             source = \"brew\"\n\
             old = \"120\"\n\
             new = \"121\"\n\
             change = \"upgraded\"\n\
             declared = true\n\n";
        twrite(history_dir.join("2026-06.toml"), toml)?;
        Ok(())
    }

    #[test]
    fn present_summary_selects_span_by_at_cursor() -> crate::Result<()> {
        // 適用後要約は `at` カーソルで catch-up span を選ぶ（nixpkgs rev / dotfiles pin ではない）。marker 無し
        // （初回 = `None`）なら全エントリを集約表示し、要約済み `at` 以降に新規が無ければ空 span になる。
        let dir = temp_dir("present-at")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        // チェーンは nA->nB（at(0)）, nB->nC（at(1)）（2 bump catch-up）。
        write_history(&dir, &[("nA", "nB"), ("nB", "nC")])?;
        // 初回（marker 無し = None）: 起点以降の 2 エントリ（2 アプリ）が集約表示される。空でないこと。
        // 非 tty 経路を `stdout_is_terminal=false` で決定論的に exercise し、pending-summary へ追記させる。
        let summarized_at = present_summary(&dir, None, PackageSourceFilter::All, false, false)?;
        let pending = tread(dir.join(PENDING_SUMMARY))?;
        assert!(
            !pending.trim().is_empty(),
            "summary must not be empty: {pending}"
        );
        assert!(pending.contains("neovim-nA"), "{pending}");
        assert!(pending.contains("neovim-nB"), "{pending}");
        // 要約し終えた終端エントリの `at`（次回カーソル）が返る。
        assert_eq!(summarized_at, Some(at_of(1)));

        // 終端 `at` 以降に新規が無ければ空 span（宣言アプリ行が出ない・marker は進めないため None が返る）。
        let dir2 = temp_dir("present-at-empty")?;
        let _ = std::fs::remove_dir_all(&dir2);
        tmkdirp(&dir2)?;
        write_history(&dir2, &[("nA", "nB"), ("nB", "nC")])?;
        let none = present_summary(
            &dir2,
            Some(at_of(1).as_str()),
            PackageSourceFilter::All,
            false,
            false,
        )?;
        let empty = std::fs::read_to_string(dir2.join(PENDING_SUMMARY)).unwrap_or_default();
        assert!(
            !empty.contains("neovim-"),
            "終端 at 以降に新規が無ければ空 span: {empty}"
        );
        assert_eq!(none, None, "空 span では次回カーソル None");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
        Ok(())
    }

    #[test]
    fn present_summary_dry_run_has_no_file_side_effect() -> crate::Result<()> {
        // dry-run 契約: 非 tty・tty いずれの経路でも `pending-summary` / `last-run.log` を書かない（副作用抑止）。
        // is_terminal() を注入化したため tty 性をテストが制御でき、`stdout_is_terminal` の両値で副作用無しを固定する。
        let dir = temp_dir("present-dry")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        write_history(&dir, &[("nA", "nB"), ("nB", "nC")])?;
        // 非 tty 経路の dry-run: pending-summary / last-run.log を書かない。
        present_summary(&dir, None, PackageSourceFilter::All, true, false)?;
        assert!(
            !dir.join(PENDING_SUMMARY).exists(),
            "dry-run (non-tty) must not write pending-summary"
        );
        assert!(
            !dir.join(LAST_RUN_LOG).exists(),
            "dry-run (non-tty) must not write last-run.log"
        );

        // tty 経路の dry-run: 端末描画のみで last-run.log も書かない（pending は tty 経路では元々書かない）。
        present_summary(&dir, None, PackageSourceFilter::All, true, true)?;
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
        let dir = temp_dir("present-tty")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        write_history(&dir, &[("nA", "nB"), ("nB", "nC")])?;
        present_summary(&dir, None, PackageSourceFilter::All, false, true)?;

        // tty 経路は pending-summary を書かない（非 tty の background 消費契約専用のため）。
        assert!(
            !dir.join(PENDING_SUMMARY).exists(),
            "tty path must not write pending-summary"
        );
        // tty 経路でも last-run.log には要約を残す（直近 1 回の適用内容を後追いできる）。
        let log = tread(dir.join(LAST_RUN_LOG))?;
        assert!(
            log.contains("neovim-nA"),
            "tty path must record summary into last-run.log: {log}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn home_only_summary_excludes_brew_cask_but_full_summary_includes_it() -> crate::Result<()> {
        // finding 運用整合 / 3368653947 退行固定: home-only catch-up（`NixOnly`）の要約は brew cask（firefox）を
        // 除外して nix（neovim）だけ出す。一方、darwin/全体適用（`All`）の要約は cask を含めて両方出す。
        // daemon フル経路では home step が NixOnly で nix だけ要約 → darwin で実適用された cask が starve する
        // 欠落を防ぐため、commit step を `All` で要約させる（その入口契約をここでフィルタ単位に固定する）。
        let dir = temp_dir("home-only-filter")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        write_history_with_nix_and_brew(&dir)?;
        // home-only（NixOnly）: neovim を出し firefox（cask）を出さない（非 tty で pending へ追記）。
        present_summary(&dir, None, PackageSourceFilter::NixOnly, false, false)?;
        let nix_only = tread(dir.join(PENDING_SUMMARY))?;
        assert!(
            nix_only.contains("neovim"),
            "NixOnly は nix を出す: {nix_only}"
        );
        assert!(
            !nix_only.contains("firefox"),
            "NixOnly は brew cask を出さない（未適用 cask 誤通知の抑止）: {nix_only}"
        );
        let _ = std::fs::remove_file(dir.join(PENDING_SUMMARY));

        // 全体/darwin（All）: 同じ span を cask 込みで要約する（commit step の適用済み cask 通知）。
        let dir2 = temp_dir("full-filter")?;
        let _ = std::fs::remove_dir_all(&dir2);
        tmkdirp(&dir2)?;
        write_history_with_nix_and_brew(&dir2)?;
        present_summary(&dir2, None, PackageSourceFilter::All, false, false)?;
        let all = tread(dir2.join(PENDING_SUMMARY))?;
        assert!(all.contains("neovim"), "All は nix を出す: {all}");
        assert!(
            all.contains("firefox"),
            "All は darwin 適用済み brew cask を出す（適用済み cask を通知する）: {all}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
        Ok(())
    }

    #[test]
    fn present_and_commit_summary_does_not_advance_marker_on_summary_error() -> crate::Result<()> {
        // finding 3368519980 退行固定（要約失敗時に marker を進めない）: `present_and_commit_summary` は要約
        // （`present_summary`）が失敗したら `last-summarized-at` を進めない。これにより、後段の apply-dedup marker
        // 確定（`commit_apply_markers`）を要約成功後に行う設計と合わせ、要約だけが失敗した場合に未表示 span を次回
        // 再 switch で再要約できる（marker が古いまま保たれる）。
        //
        // 要約失敗を hermetic に作るため、`<state>/history` 配下を **ディレクトリにできないファイル**として置く
        // （history source の読取りが失敗 → present_summary が Err）。要約前に `last-summarized-at` を at(-1) 相当の
        // 既知値へ置き、Err 後もその値が保たれることを観測する。
        let dir = temp_dir("summary-error-marker")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        // 要約前の marker（前回要約済み終端 `at`）。Err 後もこの値が変わらないことを固定する。
        write_last_summarized_at(&dir, "2026-05-31T00:00:00Z", false)?;

        // history source を「読めない」状態にする: `<state>/history` を通常ファイルにして TOML ディレクトリ走査を
        // 失敗させる（adapter のディレクトリ読取りが Err になる）。
        let history = dir.join(super::HISTORY_LOCAL_SUBDIR);
        twrite(&history, b"not a directory")?;

        // `All` scope は `last-summarized-at`（上で 2026-05-31 に確定済み）を span 起点に読む。
        let result = present_and_commit_summary(&dir, SummaryScope::All, false);
        assert!(
            result.is_err(),
            "history 読取り不能なら present_summary は Err を返す"
        );
        // 要約失敗でも marker は進めない（前回値のまま）→ 次回再 switch で未表示 span を再要約できる。
        assert_eq!(
            read_last_summarized_at(&dir)?.as_deref(),
            Some("2026-05-31T00:00:00Z"),
            "要約失敗時は last-summarized-at を進めない"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 非 tty 適用 1 回ぶん（要約 → scope カーソル確定）を、本番 `present_and_commit_summary` と同じ scope 別
    /// カーソル read/write 順序で実行する（`stdout_is_terminal=false` 注入で pending-summary 経路を決定論化）。
    ///
    /// `present_and_commit_summary` は tty 判定をアンビエント `is_terminal()` で解決するため、nix build sandbox の
    /// tty stdout でも非 tty 経路（pending-summary 追記）を確実に exercise できるよう、scope カーソルの read/write を
    /// 本番と同じく `SummaryScope` 経由で対にしつつ tty 性だけ注入する。本番 `run()` の summary 分岐と同値。
    fn apply_once_scope(state_dir: &Path, scope: SummaryScope) -> crate::Result<()> {
        let span_start_at = scope.read_cursor(state_dir)?;
        let summarized_at = present_summary(
            state_dir,
            span_start_at.as_deref(),
            scope.source_filter(),
            false,
            false,
        )?;
        if let Some(at) = summarized_at {
            scope.write_cursor(state_dir, &at, false)?;
        }
        Ok(())
    }

    #[test]
    fn summarized_home_at_marker_round_trips_and_respects_dry_run() -> crate::Result<()> {
        // `last-summarized-home-at`（home スコープ専用カーソル）の read/write 往復と dry-run 非書込を固定する。
        // `All` スコープの `last-summarized-at` とは別ファイルで分離していること（scope 分離の前提）も確認する。
        let dir = temp_dir("summarized-home-at-marker")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // marker 無し（初回）は None（全件 = 初回 catch-up）。
        assert_eq!(read_last_summarized_home_at(&dir)?, None);
        // 書込→読出で往復する。
        write_last_summarized_home_at(&dir, "2026-06-03T00:00:00Z", false)?;
        assert_eq!(
            read_last_summarized_home_at(&dir)?,
            Some("2026-06-03T00:00:00Z".to_string())
        );
        assert!(dir.join(LAST_SUMMARIZED_HOME_AT).exists());
        // `All` スコープのカーソル（last-summarized-at）とは別ファイルで、home の書込が `All` を汚さない。
        assert_eq!(
            read_last_summarized_at(&dir)?,
            None,
            "home スコープの書込は `All` スコープのカーソルを動かさない"
        );
        // dry-run は書き込まない（既存値を保つ）。
        write_last_summarized_home_at(&dir, "should-not-write", true)?;
        assert_eq!(
            read_last_summarized_home_at(&dir)?,
            Some("2026-06-03T00:00:00Z".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn daemon_home_nix_summary_does_not_starve_cask_in_commit_all_summary() -> crate::Result<()> {
        // 退行固定（運用整合 finding・要件1）: daemon（darwin）端末で zsh ログイン catch-up の home-only NixOnly
        // 要約が先に走っても、その後の daemon commit step の `All` 要約から **実適用 cask（firefox）が失われない**
        // ことを、履歴 fixture と scope カーソルのパラメータ注入で決定論的に固定する。
        //
        // 失敗インターリーブ（scope カーソルを共有していた退行）:
        //   1. home-only NixOnly 要約が neovim（nix）だけを表示し、共有カーソルを cask を含む選択 span 終端 `at`
        //      （filter 前 selected の終端）まで前進させる。
        //   2. daemon commit `All` 要約が共有カーソルを span 起点に読むため空 span（「0アプリ更新」）になり、
        //      darwin が実適用した firefox（cask）がどの pending-summary にも出ず starve する。
        // scope 別カーソル（`last-summarized-at` / `last-summarized-home-at`）にすることで、home NixOnly 要約は
        // `All` スコープのカーソルを動かさず、commit `All` 要約が cask を 1 回要約できる。
        let dir = temp_dir("daemon-cask-starve")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        // 1 エントリに nix（neovim）と brew cask（firefox）が同居する適用済み範囲（at = 2026-06-01）。
        write_history_with_nix_and_brew(&dir)?;

        // ① zsh ログイン catch-up（home-only・NixOnly）が先に要約する。neovim だけを表示し、home スコープ
        //    カーソル（`last-summarized-home-at`）だけを進める。
        apply_once_scope(&dir, SummaryScope::HomeOnlyNix)?;
        let home_pending = tread(dir.join(PENDING_SUMMARY))?;
        assert!(
            home_pending.contains("neovim"),
            "home-only NixOnly 要約は nix（neovim）を出す: {home_pending}"
        );
        assert!(
            !home_pending.contains("firefox"),
            "home-only NixOnly 要約は未適用 cask（firefox）を出さない: {home_pending}"
        );
        // home スコープカーソルだけが進み、`All` スコープのカーソルは未前進（cask starve を防ぐ核心）。
        assert_eq!(
            read_last_summarized_home_at(&dir)?.as_deref(),
            Some("2026-06-01T00:00:00Z"),
            "home NixOnly 要約は home スコープカーソルを進める"
        );
        assert_eq!(
            read_last_summarized_at(&dir)?,
            None,
            "home NixOnly 要約は `All` スコープのカーソル（last-summarized-at）を進めない"
        );
        // home 要約ぶんは消費済みとみなす（consumer が rename で消費する契約）。
        let _ = std::fs::remove_file(dir.join(PENDING_SUMMARY));

        // ② daemon commit step（`All`）が要約する。`All` スコープのカーソルは未前進なので span は空にならず、
        //    実適用 cask（firefox）を含む適用済み範囲が 1 回要約される（starve しない）。
        apply_once_scope(&dir, SummaryScope::All)?;
        let commit_pending = tread(dir.join(PENDING_SUMMARY))?;
        assert!(
            commit_pending.contains("firefox"),
            "commit `All` 要約は darwin 実適用 cask（firefox）を必ず出す（starve しない・要件1）: {commit_pending}"
        );
        assert!(
            commit_pending.contains("neovim"),
            "commit `All` 要約は同エントリの nix（neovim）も含む: {commit_pending}"
        );
        // commit 後は `All` スコープのカーソルが終端 `at` へ確定する（次回 `All` 要約は再表示しない）。
        assert_eq!(
            read_last_summarized_at(&dir)?.as_deref(),
            Some("2026-06-01T00:00:00Z"),
            "commit `All` 要約後に `All` スコープカーソルが確定する"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn home_only_terminal_does_not_show_cask_and_keeps_nix_show_once() -> crate::Result<()> {
        // 退行固定（運用整合 finding・要件2）: home-only 専用端末（daemon 無し・cask を適用しない）では、NixOnly
        // 要約だけが走る。(a) 未適用 cask（firefox）を「適用済み」として誤表示しない、(b) 同じ nix 更新（neovim）を
        // 毎ログイン再表示しない（show-once 維持）ことを、home スコープカーソルのパラメータ注入で固定する。
        let dir = temp_dir("home-only-terminal")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        // nix（neovim）と cask（firefox）が同居する 1 エントリ（at = 2026-06-01）。
        write_history_with_nix_and_brew(&dir)?;

        // 1 回目（home-only NixOnly・marker 無し = 全件）: neovim を表示し cask は出さない。home カーソルが進む。
        apply_once_scope(&dir, SummaryScope::HomeOnlyNix)?;
        let first = tread(dir.join(PENDING_SUMMARY))?;
        assert!(
            first.contains("neovim"),
            "home-only は nix を表示する: {first}"
        );
        assert!(
            !first.contains("firefox"),
            "home-only 専用端末は未適用 cask を「適用済み」と誤表示しない（要件2-a）: {first}"
        );
        assert_eq!(
            read_last_summarized_home_at(&dir)?.as_deref(),
            Some("2026-06-01T00:00:00Z"),
            "home NixOnly 要約後に home スコープカーソルが確定する"
        );
        // 表示済みぶんは消費済みとみなす。
        let _ = std::fs::remove_file(dir.join(PENDING_SUMMARY));

        // 2 回目（同一履歴・新規なし）: home カーソル以降は空 span → neovim を再表示しない（show-once・要件2-b）。
        apply_once_scope(&dir, SummaryScope::HomeOnlyNix)?;
        let second = std::fs::read_to_string(dir.join(PENDING_SUMMARY)).unwrap_or_default();
        assert!(
            !second.contains("neovim"),
            "要約済み nix 更新を毎ログイン再表示しない（show-once 維持・要件2-b）: {second:?}"
        );
        assert!(
            !second.contains("firefox"),
            "cask は home-only 専用端末で一度も表示しない: {second:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn append_pending_summary_accumulates_rev_blocks() -> crate::Result<()> {
        // 非 tty 連続適用で pending-summary が rev 単位に**追記**累積し、未表示 rev を失わないことを実ファイルで固定。
        let dir = temp_dir("pending-accumulate")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        write_history(&dir, &[("r1", "r2"), ("r2", "r3")])?;
        let source = dir.join(super::HISTORY_LOCAL_SUBDIR);

        // 1 回目: marker 無し（None = 全件）で 2 エントリ相当を 1 show として追記。終端 at(1) を返す。
        let first_cursor = append_pending_summary(&dir, &source, None, PackageSourceFilter::All)?;
        let after_first = tread(dir.join(PENDING_SUMMARY))?;
        assert!(after_first.contains("neovim-r1"));
        assert_eq!(first_cursor, Some(at_of(1)));

        // 2 回目: 別履歴に新規エントリ（r3->r4, at(2)）を足し、at(1) 起点で追記。先頭ブロックは残る（累積）。
        write_history(&dir, &[("r1", "r2"), ("r2", "r3"), ("r3", "r4")])?;
        append_pending_summary(
            &dir,
            &source,
            Some(at_of(1).as_str()),
            PackageSourceFilter::All,
        )?;
        let after_second = tread(dir.join(PENDING_SUMMARY))?;
        assert!(
            after_second.contains("neovim-r1"),
            "first block must remain: {after_second}"
        );
        assert!(
            after_second.contains("neovim-r3"),
            "new block (r3->r4) appended: {after_second}"
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
        let dir = temp_dir("defer-commit")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
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
        let base = temp_dir("copy-history")?;
        let _ = std::fs::remove_dir_all(&base);
        let source_history = base.join("src-docs-history");
        tmkdirp(&source_history)?;
        twrite(source_history.join("2026-05.toml"), "a = 1\n")?;
        twrite(source_history.join("2026-06.toml"), "b = 2\n")?;
        // サブディレクトリは複製対象外（履歴 layout 上想定しない）。
        tmkdirp(source_history.join("nested"))?;

        let dest = base.join("history");
        copy_history_dir(&source_history, &dest)?;

        assert_eq!(tread(dest.join("2026-05.toml"))?, "a = 1\n");
        assert_eq!(tread(dest.join("2026-06.toml"))?, "b = 2\n");
        // サブディレクトリはコピーされない。
        assert!(!dest.join("nested").exists());

        let _ = std::fs::remove_dir_all(&base);
        Ok(())
    }

    #[test]
    fn copy_history_dir_removes_stale_toml_no_longer_in_source() -> crate::Result<()> {
        // finding 3368677389 退行固定: source 側で月次 TOML が削除/リネームされた pin に更新したら、複製先の
        // 古い `*.toml` を残さない。上書きコピーだけだと削除済み履歴が show / 要約に混入するため、複製前に dest を
        // 作り直して source に無いファイルを消す。
        let base = temp_dir("copy-history-stale")?;
        let _ = std::fs::remove_dir_all(&base);
        let source_history = base.join("src-docs-history");
        let dest = base.join("history");
        tmkdirp(&source_history)?;

        // 1 回目の pin: 2026-05 と 2026-06 を複製する。
        twrite(source_history.join("2026-05.toml"), "a = 1\n")?;
        twrite(source_history.join("2026-06.toml"), "b = 2\n")?;
        copy_history_dir(&source_history, &dest)?;
        assert!(dest.join("2026-05.toml").exists());
        assert!(dest.join("2026-06.toml").exists());

        // 2 回目の pin: source 側で 2026-05 を削除（リネーム相当）し 2026-06 を更新する。
        tremove_file(source_history.join("2026-05.toml"))?;
        twrite(source_history.join("2026-06.toml"), "b = 22\n")?;
        copy_history_dir(&source_history, &dest)?;

        // 削除済み 2026-05 は複製先からも消えている（古い履歴が表示に混入しない）。
        assert!(
            !dest.join("2026-05.toml").exists(),
            "source から消えた古い TOML は複製先からも除去される"
        );
        // 残った 2026-06 は新しい内容で更新されている。
        assert_eq!(tread(dest.join("2026-06.toml"))?, "b = 22\n");
        // sync temp の残骸が残らない（atomic 置換後に消える）。
        let leftover_temp: Vec<_> = std::fs::read_dir(&base)
            .map_err(|error| anyhow!("read base: {error}"))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".sync."))
            .collect();
        assert!(leftover_temp.is_empty(), "sync temp dir must not linger");

        let _ = std::fs::remove_dir_all(&base);
        Ok(())
    }

    #[test]
    fn replace_history_dir_atomically_preserves_old_replica_when_rename_fails() -> crate::Result<()>
    {
        // finding 3374863441 退行固定: temp→dest の置換 rename が失敗しても、既存の旧複製を喪失しない。
        // 旧実装は dest を先に remove_dir_all してから rename したため、rename 失敗時に旧複製が消え、
        // 呼び出し側が警告に落として last-applied-* を確定すると次回同じ pin で switch/sync が skip され、
        // show / 要約が空のまま自己回復しなかった。新実装は dest を backup へ退避し、置換失敗時に backup を
        // dest へ rename 復元するため旧複製が残る。
        //
        // 置換 rename の失敗を決定論的に注入する: `temp_dir` を **存在しないパス**にすると `fs::rename(temp, dest)`
        // が `NotFound` で失敗し、置換失敗経路（temp 掃除 → backup→dest 復元）を確実に通る。退行版（dest を先に
        // remove_dir_all して rename）ならこの注入で旧複製が消え、本テストの「旧複製温存」assert が FAIL する。
        let base = temp_dir("replace-history-preserve")?;
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).map_err(|error| anyhow!("create base: {error}"))?;
        let dest = base.join("history");
        let backup_dir = base.join("history.backup.tmp");

        // 旧複製（失われてはならない既存複製）を dest に用意する。
        std::fs::create_dir_all(&dest).map_err(|error| anyhow!("create dest: {error}"))?;
        twrite(dest.join("2026-05.toml"), "old = 1\n")
            .map_err(|error| anyhow!("write old toml: {error}"))?;

        // 置換元 temp を存在しないパスにして temp→dest rename を必ず失敗させる。
        let missing_temp = base.join("history.sync.missing.tmp");
        let result = replace_history_dir_atomically(&missing_temp, &dest, &backup_dir);
        if result.is_ok() {
            return Err(anyhow!(
                "replacement must fail when the prepared temp dir is absent"
            ));
        }

        // 旧複製が dest に残り、内容も元のまま（backup→dest へ復元される）であること。
        let preserved = std::fs::read_to_string(dest.join("2026-05.toml"))
            .map_err(|error| anyhow!("old replica must survive failed replacement: {error}"))?;
        assert_eq!(
            preserved, "old = 1\n",
            "置換失敗時は旧複製を喪失せず元内容のまま温存する"
        );
        // backup 退避先が残骸として残らない（復元 rename で dest へ戻る）。
        assert!(
            !backup_dir.exists(),
            "復元後に backup 退避先が残骸として残らない"
        );

        let _ = std::fs::remove_dir_all(&base);
        Ok(())
    }

    /// 非 tty 適用 1 回ぶんを、履歴複製の成否（`history_synced`）で要約・marker 確定を gate して実行する。
    ///
    /// 本番 `run()` は `sync_history` 成功時だけ present_summary → `last-summarized-at` 確定へ進む（A）。
    /// 失敗時は要約も marker 確定もせず span 起点（`at` カーソル）を保つ。nix を伴わずにこの gate 挙動を固定する
    /// ため、`history_synced` を引数で注入して run() の該当分岐と同じ順序を再現する。
    fn apply_once_defer_gated(state_dir: &Path, history_synced: bool) -> crate::Result<()> {
        if history_synced {
            let span_start_at = read_last_summarized_at(state_dir)?;
            let summarized_at = present_summary(
                state_dir,
                span_start_at.as_deref(),
                PackageSourceFilter::All,
                false,
                false,
            )?;
            if let Some(at) = summarized_at {
                write_last_summarized_at(state_dir, &at, false)?;
            }
        }
        Ok(())
    }

    #[test]
    fn sync_history_failure_does_not_advance_summarized_marker() -> crate::Result<()> {
        // A 退行固定: sync_history が失敗（履歴複製が無い）した適用では、要約も `last-summarized-at` の確定も
        // しない。これにより、その範囲の要約が永久に失われる（marker だけ進んで次回再表示されない）退行を防ぐ。
        // 次回 sync 成功時の再実行で未表示範囲を再び要約できる。
        let dir = temp_dir("sync-fail-marker")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        // 履歴 chain: N0->N1（at(0)）。
        write_history(&dir, &[("N0", "N1")])?;
        // 履歴複製失敗（history_synced=false）の適用: 要約も marker 確定もしない。
        apply_once_defer_gated(&dir, false)?;
        assert!(
            !dir.join(PENDING_SUMMARY).exists(),
            "sync 失敗時は要約（pending-summary）を書かない"
        );
        assert_eq!(
            read_last_summarized_at(&dir)?,
            None,
            "sync 失敗時は要約済み marker を進めない（その範囲の要約を失わない）"
        );

        // 次回（履歴複製成功）で同じ範囲 N0->N1 を要約でき、marker が終端 at(0) へ進む（未表示範囲を取り戻す）。
        apply_once_defer_gated(&dir, true)?;
        let pending = tread(dir.join(PENDING_SUMMARY))?;
        assert!(
            pending.contains("neovim-N0"),
            "再実行で未表示範囲 N0->N1 を要約する: {pending}"
        );
        assert_eq!(
            read_last_summarized_at(&dir)?,
            Some(at_of(0)),
            "sync 成功後に marker が確定する"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn sync_history_treats_archive_failure_as_unsynced_error() -> crate::Result<()> {
        // finding 3368519977 退行固定: `nix flake archive` 失敗（`resolve_input_source` が `None`）は同期
        // 未成功として `Err` を返し、呼び出し側が `history_synced = true` にして空/古い履歴で marker を進める
        // 退行を防ぐ。source あり履歴無しは「複製対象が無い正常系」として `Ok(())`（archive 失敗と区別する）。
        let dir = temp_dir("sync-archive-fail")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // archive 失敗（None）: Err を返す（marker 確定を抑止できるよう同期未成功にする）。
        assert!(
            sync_history_from_source(None, &dir).is_err(),
            "archive 失敗（source 解決不能）は同期未成功として Err にする"
        );

        // source は解決できたが `docs/update-history` が無い: 複製対象が無いだけの正常系として Ok。
        let source_without_history = dir.join("src-no-history");
        tmkdirp(&source_without_history)?;
        assert!(
            sync_history_from_source(Some(source_without_history), &dir).is_ok(),
            "source あり履歴無しは複製対象が無いだけで Ok（archive 失敗と区別する）"
        );

        // source 解決成功 + 履歴 dir あり: 複製して Ok。
        let source_with_history = dir.join("src-with-history");
        let history = source_with_history.join(super::HISTORY_SUBDIR);
        tmkdirp(&history)?;
        twrite(history.join("2026-06.toml"), "x = 1\n")?;
        sync_history_from_source(Some(source_with_history), &dir)?;
        assert_eq!(
            tread(dir.join(super::HISTORY_LOCAL_SUBDIR).join("2026-06.toml"))?,
            "x = 1\n",
            "履歴 dir があれば state dir のローカル複製へコピーする"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn deferred_rev_round_trips_and_commit_uses_deferred_pin() -> crate::Result<()> {
        // B 退行固定: defer 時に控えた pin/nixpkgs rev を commit が確定する（commit 時に現在 pin を読み直さない）。
        // run() の home/darwin 二段は実適用を要するため、ここでは commit が参照する defer marker の I/O 契約
        // （round-trip・read_deferred 優先・dotfiles pin / nixpkgs の独立保持・dry-run 非書込）を直接固定する。
        let dir = temp_dir("deferred-rev")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

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
    fn commit_token_match_confirms_deferred_pin_mismatch_skips() {
        // finding 3368519975 退行固定（純粋判定）: commit の token 一致検証。
        //
        // ケース1: 渡された token == stored token → このサイクルの defer 値（pin/nixpkgs）を確定する。
        match resolve_committed_marker(
            Some("cycle-1"),
            Some("cycle-1"),
            Some("pin-applied-by-root"),
            Some("nixpkgs-applied-by-root"),
        ) {
            CommitDecision::Confirm { pin, nixpkgs_rev } => {
                assert_eq!(pin, Some("pin-applied-by-root"));
                assert_eq!(nixpkgs_rev, Some("nixpkgs-applied-by-root"));
            }
            CommitDecision::Skip => panic!("token 一致時は確定すべき"),
        }

        // ケース2: 渡された token != stored token（別 catch-up サイクルが darwin 実行中に deferred 値を上書き）→
        // **確定を skip**。root が適用していない後続サイクルの pin を `last-applied` へ確定しない。
        assert!(matches!(
            resolve_committed_marker(
                Some("cycle-1"),
                Some("cycle-2-overwrote-it"),
                Some("pin-from-later-cycle"),
                Some("nixpkgs-from-later-cycle"),
            ),
            CommitDecision::Skip
        ));

        // ケース3: token を渡したが stored token 不在（defer が token を控えていない / 別サイクルが clear）→ Skip。
        assert!(matches!(
            resolve_committed_marker(Some("cycle-1"), None, Some("some-pin"), None),
            CommitDecision::Skip
        ));

        // ケース4: token 無し（後方互換・旧ラッパー・defer を経ない直接 commit）→ deferred 値があれば確定、
        // 無ければ現在値縮退（pin None）。token 検証は要求されないので skip しない。
        match resolve_committed_marker(None, None, Some("deferred-pin"), Some("deferred-nixpkgs")) {
            CommitDecision::Confirm { pin, nixpkgs_rev } => {
                assert_eq!(pin, Some("deferred-pin"));
                assert_eq!(nixpkgs_rev, Some("deferred-nixpkgs"));
            }
            CommitDecision::Skip => panic!("token 無し経路は従来どおり確定する"),
        }
        match resolve_committed_marker(None, None, None, None) {
            CommitDecision::Confirm { pin, nixpkgs_rev } => {
                assert_eq!(pin, None, "deferred 値も無ければ現在 pin 縮退（None）");
                assert_eq!(nixpkgs_rev, None);
            }
            CommitDecision::Skip => panic!("token 無し・deferred 無しは現在値縮退で確定する"),
        }
    }

    #[test]
    fn commit_writeback_gating_confirms_only_on_summary_success() -> crate::Result<()> {
        // finding 3376248504 退行固定: commit（`--commit-rev-marker`）Confirm 分岐の writeback gating。
        //
        // 純粋判定: 要約成功 → `Persist`（rev marker 確定 + deferred clear）、要約失敗 → `Defer`（rev 未確定 +
        // deferred 残置）。要約成否を bool で注入し、両分岐を I/O・network・nix 無しで決定論的に固定する。
        assert_eq!(commit_writeback_plan(true), CommitWriteback::Persist);
        assert_eq!(commit_writeback_plan(false), CommitWriteback::Defer);

        // I/O 契約: 純粋判定を run() と同じ順序で marker 書込みへ結線したとき、要約成否で state file が
        // 取り違わずに確定/残置されることを固定する（gating が退行すると未表示 span を失う）。
        // ケース失敗（Defer）: deferred marker を控えた状態で要約失敗 → rev 未確定・deferred 残置。
        let fail_dir = temp_dir("commit-writeback-defer")?;
        let _ = std::fs::remove_dir_all(&fail_dir);
        tmkdirp(&fail_dir)?;
        write_deferred_rev(&fail_dir, "deferred-pin", false)?;
        write_deferred_nixpkgs_rev(&fail_dir, "deferred-nixpkgs", false)?;
        match commit_writeback_plan(false) {
            CommitWriteback::Persist => {
                write_last_applied_rev(&fail_dir, "deferred-pin", false)?;
                write_last_applied_nixpkgs_rev(&fail_dir, "deferred-nixpkgs", false)?;
                clear_deferred_markers(&fail_dir, false);
            }
            CommitWriteback::Defer => {}
        }
        assert_eq!(
            read_last_applied_rev(&fail_dir)?,
            None,
            "要約失敗時は rev を確定しない"
        );
        assert_eq!(
            read_deferred_rev(&fail_dir)?.as_deref(),
            Some("deferred-pin"),
            "要約失敗時は deferred marker を残置する（次サイクルで再要約）"
        );
        assert_eq!(
            read_deferred_nixpkgs_rev(&fail_dir)?.as_deref(),
            Some("deferred-nixpkgs"),
            "要約失敗時は deferred nixpkgs marker も残置する"
        );
        let _ = std::fs::remove_dir_all(&fail_dir);

        // ケース成功（Persist）: 同じ初期状態で要約成功 → rev 確定 + deferred clear。
        let ok_dir = temp_dir("commit-writeback-persist")?;
        let _ = std::fs::remove_dir_all(&ok_dir);
        tmkdirp(&ok_dir)?;
        write_deferred_rev(&ok_dir, "deferred-pin", false)?;
        write_deferred_nixpkgs_rev(&ok_dir, "deferred-nixpkgs", false)?;
        match commit_writeback_plan(true) {
            CommitWriteback::Persist => {
                write_last_applied_rev(&ok_dir, "deferred-pin", false)?;
                write_last_applied_nixpkgs_rev(&ok_dir, "deferred-nixpkgs", false)?;
                clear_deferred_markers(&ok_dir, false);
            }
            CommitWriteback::Defer => {}
        }
        assert_eq!(
            read_last_applied_rev(&ok_dir)?.as_deref(),
            Some("deferred-pin"),
            "要約成功時は defer 時点の pin を確定する"
        );
        assert_eq!(
            read_deferred_rev(&ok_dir)?,
            None,
            "要約成功時は deferred marker を clear する"
        );
        assert_eq!(
            read_deferred_nixpkgs_rev(&ok_dir)?,
            None,
            "要約成功時は deferred nixpkgs marker も clear する"
        );
        let _ = std::fs::remove_dir_all(&ok_dir);

        Ok(())
    }

    #[test]
    fn deferred_token_round_trips_and_clears_with_markers() -> crate::Result<()> {
        // finding 3368519975 退行固定（I/O 契約）: defer 時に控える `deferred-token` の read/write 往復と、
        // `clear_deferred_markers` が token も pin/nixpkgs rev と一緒に 1 サイクルへ閉じる（消す）ことを固定する。
        let dir = temp_dir("deferred-token")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // defer 前は token 不在。
        assert_eq!(read_deferred_token(&dir)?, None);
        // defer 時にサイクル token を控える（pin/nixpkgs と同じ瞬間に書く想定）。
        write_deferred_rev(&dir, "pin-x", false)?;
        write_deferred_nixpkgs_rev(&dir, "nixpkgs-x", false)?;
        write_deferred_token(&dir, "cycle-token-abc", false)?;
        assert_eq!(
            read_deferred_token(&dir)?,
            Some("cycle-token-abc".to_string())
        );
        assert!(dir.join(DEFERRED_TOKEN).exists());

        // dry-run は控えを書かない（既存値を壊さない）。
        write_deferred_token(&dir, "should-not-write", true)?;
        assert_eq!(
            read_deferred_token(&dir)?,
            Some("cycle-token-abc".to_string())
        );

        // clear は pin/nixpkgs/token を 1 サイクルへ閉じる（すべて消える）。
        clear_deferred_markers(&dir, false);
        assert_eq!(
            read_deferred_token(&dir)?,
            None,
            "token もサイクルローカルに消える"
        );
        assert_eq!(read_deferred_rev(&dir)?, None);
        assert_eq!(read_deferred_nixpkgs_rev(&dir)?, None);
        assert!(!dir.join(DEFERRED_TOKEN).exists());
        assert!(!dir.join(DEFERRED_REV).exists());
        assert!(!dir.join(DEFERRED_NIXPKGS_REV).exists());

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
    fn full_switch_decision_uses_whole_lock_identity_not_just_pin() {
        // finding 3368636842 退行固定: `--full` は全入力更新を宣言するため、dotfiles pin が不変でも nixpkgs/
        // framework など他 input だけが動いたら switch する。pin だけ比較する should_switch では skip してしまう
        // ケースを、lock 全体 identity の比較で拾う。
        //
        // ケース1: pin 不変・lock 全体 identity 変化（他 input だけ動いた）→ switch する。
        assert!(
            should_switch_full(Some("pin-same"), "pin-same", Some("lock-old"), "lock-new"),
            "pin 不変でも lock 全体が変われば --full は switch する"
        );
        // 対照: pin だけ見る should_switch は同じ状況を skip する（pin 単独判定の欠陥）。
        assert!(
            !should_switch(Some("pin-same"), "pin-same"),
            "pin 単独では pin 不変を skip する（lock 全体判定が必要な根拠）"
        );
        // ケース2: pin も lock 全体も不変 → skip する。
        assert!(
            !should_switch_full(Some("pin-same"), "pin-same", Some("lock-same"), "lock-same"),
            "pin も lock 全体も不変なら --full でも skip する"
        );
        // ケース3: pin 変化（lock も当然変化）→ switch する。
        assert!(
            should_switch_full(Some("pin-old"), "pin-new", Some("lock-old"), "lock-new"),
            "pin 変化は switch する"
        );
        // ケース4: lock-id marker 無し（--full 初回・本機能導入前）→ lock 未適用とみなして switch する。
        assert!(
            should_switch_full(Some("pin-same"), "pin-same", None, "lock-new"),
            "lock-id marker 無し（初回）は switch する"
        );
    }

    #[test]
    fn lock_content_id_is_deterministic_and_changes_on_any_byte() {
        // lock 全体ダイジェストが決定論的で、1 バイトでも変われば id が動くことを固定する（他 input 変化の検知根拠）。
        let lock_a =
            br#"{"nodes":{"dotfiles":{"locked":{"rev":"r"}},"nixpkgs":{"locked":{"rev":"n1"}}}}"#;
        let lock_b =
            br#"{"nodes":{"dotfiles":{"locked":{"rev":"r"}},"nixpkgs":{"locked":{"rev":"n2"}}}}"#;
        // 同じバイト列は同じ id（決定論）。
        assert_eq!(lock_content_id(lock_a), lock_content_id(lock_a));
        // dotfiles rev は同じ（"r"）だが nixpkgs rev だけ違う → id が変わる（pin では捉えられない変化を捉える）。
        assert_ne!(
            lock_content_id(lock_a),
            lock_content_id(lock_b),
            "dotfiles pin 不変でも nixpkgs だけ変われば lock id は変わる"
        );
        // 16 桁の 16 進文字列を返す。
        assert_eq!(lock_content_id(lock_a).len(), 16);
    }

    #[test]
    fn last_applied_lock_id_round_trips_and_respects_dry_run() -> crate::Result<()> {
        // `--full` 専用 marker `last-applied-lock-id` の read/write 往復と dry-run 非書込を固定する。
        let dir = temp_dir("applied-lock-id")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // marker 無し（--full 初回）は None。
        assert_eq!(read_last_applied_lock_id(&dir)?, None);
        // 書込→読出で往復する。
        write_last_applied_lock_id(&dir, "deadbeefdeadbeef", false)?;
        assert_eq!(
            read_last_applied_lock_id(&dir)?,
            Some("deadbeefdeadbeef".to_string())
        );
        assert!(dir.join(LAST_APPLIED_LOCK_ID).exists());
        // dry-run は書かない（既存値を保つ）。
        write_last_applied_lock_id(&dir, "should-not-write", true)?;
        assert_eq!(
            read_last_applied_lock_id(&dir)?,
            Some("deadbeefdeadbeef".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn append_pending_summary_does_not_publish_partial_block_on_render_failure() -> crate::Result<()>
    {
        // C 退行固定: render 途中失敗で部分的な pending-summary を公開・消費させない。完成済みブロックだけを
        // temp 経由で 1 回 write する設計のため、履歴 source が壊れて render に失敗しても pending-summary は
        // 作られない（既存内容も汚さない）。ここでは履歴複製を欠いた state dir（render が空/失敗しうる source）で
        // 既存内容が温存され、temp ファイルが残骸として残らないことを固定する。
        let dir = temp_dir("pending-atomic")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

        // 既存 pending-summary に確定済みブロックがある状態を作る（marker 無し = None で全件要約）。
        write_history(&dir, &[("r1", "r2")])?;
        let source = dir.join(super::HISTORY_LOCAL_SUBDIR);
        append_pending_summary(&dir, &source, None, PackageSourceFilter::All)?;
        let baseline = tread(dir.join(PENDING_SUMMARY))?;
        assert!(baseline.contains("neovim-r1"), "baseline block present");

        // publish/claim 用 temp が残骸として残っていないこと（成功時も掃除する契約）。
        let temp_leftovers: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|error| anyhow!("read state dir: {error}"))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with(&format!("{PENDING_SUMMARY}.publish."))
                    || name.starts_with(&format!("{PENDING_SUMMARY}.appending."))
            })
            .collect();
        assert!(
            temp_leftovers.is_empty(),
            "publish/claim temp must not linger after append"
        );

        // 存在しない source（render が空ブロックになる）でも、既存の確定済みブロックは温存される
        // （部分公開・既存破壊が起きない）。
        let missing_source = dir.join("does-not-exist");
        append_pending_summary(&dir, &missing_source, Some("r1"), PackageSourceFilter::All)?;
        let after = tread(dir.join(PENDING_SUMMARY))?;
        assert!(
            after.contains("neovim-r1"),
            "existing committed block must be preserved: {after}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn append_pending_summary_does_not_lose_block_when_consumer_renames_concurrently()
    -> crate::Result<()> {
        // finding 3368519974 退行固定: producer の publish 中に consumer（zsh）が `mv "$pending"
        // "$pending.consuming.$$"` で消費しても要約ブロックを失わない。producer は consumer と同じ rename で
        // 所有権を取って publish するため、consumer が先に claim した既存ブロックは consumer 側に残り、producer は
        // NotFound → 新ブロックだけを fresh publish する（孤児 inode への append で要約が消えない）。
        let dir = temp_dir("pending-consumer-race")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        let pending = dir.join(PENDING_SUMMARY);

        // 履歴 chain: r1->r2（at(0)）, r2->r3（at(1)）。
        write_history(&dir, &[("r1", "r2"), ("r2", "r3")])?;
        let source = dir.join(super::HISTORY_LOCAL_SUBDIR);

        // producer #1: ブロック A（r1->r2..）を publish する（marker 無し → 全件だが limit 無しなので両方。
        // ここでは 1 件目 at(0) 起点の累積を見るため None で全件 publish）。
        append_pending_summary(&dir, &source, None, PackageSourceFilter::All)?;
        let block_a = tread(&pending)?;
        assert!(
            block_a.contains("neovim-r1"),
            "block A published: {block_a}"
        );

        // consumer が pending を atomic rename で claim する（producer #2 の publish 直前の窓を模す）。
        let consuming = dir.join(format!("{PENDING_SUMMARY}.consuming.test"));
        trename(&pending, &consuming)?;
        assert!(!pending.exists(), "consumer took the pending file");

        // producer #2: 新たな履歴 r3->r4（at(2)）を足して publish する。pending は consumer が持ち去ったので
        // producer の claim は NotFound → block B だけを fresh publish する（既存 inode へ append しない）。
        write_history(&dir, &[("r1", "r2"), ("r2", "r3"), ("r3", "r4")])?;
        append_pending_summary(
            &dir,
            &source,
            Some(at_of(1).as_str()),
            PackageSourceFilter::All,
        )?;

        // consumer が claim した分（block A）は失われていない。
        let consumed = tread(&consuming)?;
        assert!(
            consumed.contains("neovim-r1"),
            "consumer-claimed block A must survive the concurrent producer publish: {consumed}"
        );
        // producer #2 の新ブロック B も新しい pending として公開されている（孤児 inode へ書いて失っていない）。
        let block_b = tread(&pending)?;
        assert!(
            block_b.contains("neovim-r3"),
            "new block B must be published to a fresh pending, not lost to the orphaned inode: {block_b}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn concurrent_stale_steal_yields_single_winner_via_rename_cas() -> crate::Result<()> {
        // D 退行固定（B 是正: ファイル protocol レベルで単一勝者を検証）: stale lock を複数の競合者が同時奪取しても、
        // rename ベースの CAS で **同時に保持される lock は高々 1 つ**になる。旧実装（read→remove_file→create_new）は
        // A の新 lock を B の remove が消し、双方が create_new に成功して二重奪取・二重適用しうる race があった。
        //
        // **`STEAL_SECTION_MUTEX` を経由しない**: 旧テストは `try_acquire`→`steal_stale_lock` 経由で奪取区間が
        // process-wide mutex に直列化され、rename-CAS をバグ版（lock path を直接 remove→create_new）へ退行させても
        // pass してしまい（protocol の単一勝者性を検証していない）、隠蔽されていた。ここでは奪取区間内側の **ファイル
        // primitive** [`UpdateLock::steal_stale_lock_file_via_rename`] を多スレッドから直接競わせ、mutex を経由せずに
        // **ファイル protocol そのものが cross-thread（= cross-process と同じ create_new(O_EXCL)/rename CAS）で単一
        // 勝者を保証する**ことを固定する。リトマス: rename-CAS を旧バグ（remove→create_new）へ退行させると本テストが
        // FAIL する（同時保持が 2 以上になる）。
        //
        // 取得できた lock を **解放せず保持し続けたまま** 全スレッドの完了を待つ。誰も解放しないので、同時保持数が
        // そのまま「同時に成立した排他の数」になる。これが 1 を超えれば二重奪取＝退行。
        let dir = temp_dir("steal-cas")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        let lock_path = dir.join(LOCK_FILE);

        let contenders = 16;
        for _ in 0..64 {
            // 各ラウンドで古い孤児 lock（owner pid 不在 + 古い timestamp）を置き、全スレッドに stale な被奪取
            // 対象を 1 本だけ与える。pid 生存ガードを確実に通すため、回収済み（dead な）pid を使う。
            let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 120);
            twrite(
                &lock_path,
                dead_lock_payload(dead_pid_for_test(), stale_epoch),
            )?;

            let barrier = std::sync::Arc::new(std::sync::Barrier::new(contenders));
            // 取得できた lock を解放せず保持し続けるための退避先（同時保持数を測る）。
            let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::<UpdateLock>::new()));
            std::thread::scope(|scope| {
                for _ in 0..contenders {
                    let lock_path = lock_path.clone();
                    let barrier = std::sync::Arc::clone(&barrier);
                    let held = std::sync::Arc::clone(&held);
                    scope.spawn(move || {
                        barrier.wait();
                        // **mutex を経由せず**ファイル primitive を直接競わせる（protocol レベルの単一勝者検証）。
                        if let Ok(Some(lock)) =
                            UpdateLock::steal_stale_lock_file_via_rename(&lock_path)
                        {
                            // 解放せず保持する（scope 終了まで生かして同時保持数を観測する）。
                            if let Ok(mut guard) = held.lock() {
                                guard.push(lock);
                            }
                        }
                    });
                }
            });
            let mut held = held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = held.len();
            assert!(
                count <= 1,
                "stale lock steal must yield at most one concurrently-held lock, got {count}"
            );
            // 保持していた lock を解放してから次ラウンドへ（drop で lock ファイルを除去）。
            held.clear();
            drop(held);
            // 奪取の rename 中継（`update.lock.stealing.*`）が残骸として残らない。
            let leftover_stealing = std::fs::read_dir(&dir)
                .map_err(|error| anyhow!("read dir: {error}"))?
                .filter_map(std::result::Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".stealing."));
            assert!(
                !leftover_stealing,
                "rename-CAS steal middle files must not leak"
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
        let dir = temp_dir("orphan-steal")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        let lock_path = dir.join(LOCK_FILE);
        let steal_marker = dir.join(format!("{LOCK_FILE}.steal"));

        // stale な孤児 lock（Drop されず残った残骸、owner pid 不在）を置く。
        let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 120);
        twrite(
            &lock_path,
            dead_lock_payload(dead_pid_for_test(), stale_epoch),
        )?;

        // 孤児 steal marker（TTL 超過 + owner pid 不在）を置く。これが残ると旧実装は AlreadyExists で永久 skip
        // した。pid 生存ガードを通すため dead な pid を使う。
        let stale_marker_epoch =
            super::now_epoch_secs().saturating_sub(STEAL_MARKER_STALE_SECS + 60);
        twrite(
            &steal_marker,
            dead_lock_payload(dead_pid_for_test(), stale_marker_epoch),
        )?;

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
        let dir = temp_dir("fresh-steal")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        let lock_path = dir.join(LOCK_FILE);
        let steal_marker = dir.join(format!("{LOCK_FILE}.steal"));

        // stale lock（owner pid 不在）+ 新鮮な steal marker（別プロセスが今まさに奪取中）。marker は
        // timestamp が新鮮なので回収されない（pid に依らず）。
        let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 120);
        twrite(
            &lock_path,
            dead_lock_payload(dead_pid_for_test(), stale_epoch),
        )?;
        twrite(&steal_marker, live_lock_payload(super::now_epoch_secs()))?;

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
    fn concurrent_orphan_steal_marker_reclaim_yields_single_winner_via_rename_cas()
    -> crate::Result<()> {
        // finding 3368585963 退行固定（B 是正: ファイル protocol レベルで単一勝者を検証）: steal marker の
        // **TTL 回収経路**（孤児 marker の期限切れ回収）も rename ベース CAS で単一勝者になることを直接固定する。
        // 旧実装の「無条件 remove_file → create_new」では、複数プロセスが同時に孤児 marker を観測すると、A が
        // remove→create_new で新 marker を張った直後に (孤児を観測済みの) B が remove で A の新 marker を消し B も
        // create_new に成功して、A・B 双方が奪取区間へ入り 2 本の dotfiles update が同時に switch/marker 更新へ
        // 進みうる。reclaim を rename CAS にしたため、孤児を一意名へ rename で奪えた 1 人だけが回収者になり、
        // 同時に保持される lock は高々 1 つになる。
        //
        // **`STEAL_SECTION_MUTEX` を経由しない**: 奪取区間のファイル protocol 本体 [`UpdateLock::steal_stale_lock_protocol`]
        // を多スレッドから直接競わせ、in-process mutex の直列化に隠蔽されずに **protocol（marker reclaim の rename CAS
        // ＋実 lock の rename CAS）そのものが cross-process（= create_new(O_EXCL)/rename）で単一勝者を保証する**ことを
        // 固定する。リトマス: rename-CAS を旧バグ（remove→create_new）へ退行させると本テストが FAIL する。
        //
        // 各ラウンドで「stale lock + 孤児 steal marker（TTL 超過 + dead pid）」を置き、全スレッドに **TTL 回収経路**
        // を通らせる（毎回 marker を置くので create_new は AlreadyExists で必ず reclaim 分岐へ入る）。奪取できた
        // lock を解放せず保持して同時保持数を測り、1 を超えれば二重奪取＝退行。
        let dir = temp_dir("orphan-steal-cas")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;
        let lock_path = dir.join(LOCK_FILE);
        let steal_marker = dir.join(format!("{LOCK_FILE}.steal"));

        let contenders = 16;
        for _ in 0..64 {
            // stale な孤児 lock（dead pid + 古い timestamp）。
            let stale_epoch = super::now_epoch_secs().saturating_sub(LOCK_STALE_SECS + 120);
            twrite(
                &lock_path,
                dead_lock_payload(dead_pid_for_test(), stale_epoch),
            )?;
            // 孤児 steal marker（TTL 超過 + dead pid）。これにより全スレッドが create_new=AlreadyExists →
            // steal_marker_is_stale=true → reclaim_stale_steal_marker（rename CAS）の TTL 回収経路を通る。
            let stale_marker_epoch =
                super::now_epoch_secs().saturating_sub(STEAL_MARKER_STALE_SECS + 60);
            twrite(
                &steal_marker,
                dead_lock_payload(dead_pid_for_test(), stale_marker_epoch),
            )?;

            let barrier = std::sync::Arc::new(std::sync::Barrier::new(contenders));
            let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::<UpdateLock>::new()));
            std::thread::scope(|scope| {
                for _ in 0..contenders {
                    let lock_path = lock_path.clone();
                    let barrier = std::sync::Arc::clone(&barrier);
                    let held = std::sync::Arc::clone(&held);
                    scope.spawn(move || {
                        barrier.wait();
                        // **mutex を経由せず**奪取区間のファイル protocol 本体を直接競わせる（reclaim CAS ＋ steal CAS の
                        // 単一勝者性を protocol レベルで検証する）。
                        if let Ok(Some(lock)) = UpdateLock::steal_stale_lock_protocol(&lock_path) {
                            // 解放せず保持して同時保持数を観測する。
                            if let Ok(mut guard) = held.lock() {
                                guard.push(lock);
                            }
                        }
                    });
                }
            });
            let mut held = held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = held.len();
            assert!(
                count <= 1,
                "orphan steal marker TTL reclaim must yield at most one concurrently-held lock, got {count}"
            );
            held.clear();
            drop(held);
            // 奪取区間終了で marker は除去されている（回収中継 reclaiming ファイルも残さない）。
            assert!(
                !steal_marker.exists(),
                "steal marker must be cleaned up after the reclaim/steal section"
            );
            let leftover_reclaiming = std::fs::read_dir(&dir)
                .map_err(|error| anyhow!("read dir: {error}"))?
                .filter_map(std::result::Result::ok)
                .any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .contains(".steal.reclaiming.")
                });
            assert!(
                !leftover_reclaiming,
                "rename-CAS reclaim middle files must not leak"
            );
            let _ = std::fs::remove_file(&lock_path);
        }

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
        let dir = temp_dir("cycle-local-defer")?;
        let _ = std::fs::remove_dir_all(&dir);
        tmkdirp(&dir)?;

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

# nightly bump 後に各マシンを repo pin へ無人で収束させる auto-update daemon。
#
# nix-darwin ネイティブの `launchd.daemons`（root/system。`launchagents`＝user ではない）として
# `StartCalendarInterval` で定期起動し、権限分割ラッパー経由で `dotfiles` CLI を呼ぶ。ラッパーは
# 権限分割のみを担う薄い層で、適用要否判定・home-manager 適用・要約/マーカー書込みといった業務判断は
# すべて `dotfiles` CLI 側にあり、nix へは漏らさない。
#
# 権限分割（最重要）:
#   - home-manager 適用とユーザ状態（`last-applied-rev` / `pending-summary` / `last-run.log` /
#     `update.lock`）の書込みは `sudo -u <user> dotfiles update <user-targets>` でユーザ権限で行う。
#     home-manager を root で走らせず、状態マーカーをユーザ所有で書くことを厳守する。
#   - darwin-rebuild は root のまま `DOTFILES_DARWIN_REBUILD_SUDO=0 dotfiles switch darwin` で適用する
#     （既に root のため sudo を前置しない経路）。
#
# rev マーカー確定のタイミング（drift 防止）:
#   home と darwin は別 CLI 起動で適用するため、home ステップでは `--defer-rev-marker` を付けて
#   `last-applied-rev` をまだ書かない。home+darwin の両適用が成功した後にだけ `--commit-rev-marker` で
#   rev を確定する。darwin が失敗すると（`set -e`）確定ステップへ到達せず、マーカーは前回値のまま残るので、
#   次回起動で再適用して収束する（darwin 未収束のまま「適用済み」と誤記録して skip する drift を防ぐ）。
#
# defer→commit のサイクル固定（rev マーカー token。finding 3368519975）:
#   home の `--defer-rev-marker` 終了後に user 側 `update.lock` を解放してから root の darwin を別プロセスで
#   実行する間、別の login catch-up が新サイクルを始めて `deferred-rev` を上書きでき、commit が root の適用した
#   pin ではなく後続サイクルの pin を確定して以後の darwin 適用を skip し得る（darwin 実行中は lock 解放済みの
#   ため排他で防げない）。これを防ぐため、wrapper は defer 直前に 1 サイクル分の token（`rev_marker_token`）を
#   生成し、home defer ステップと commit ステップへ **同値** を `--rev-marker-token` で渡す。CLI は defer 時に
#   deferred 値と同じ瞬間にこの token を `deferred-token` へ控え、commit 時に `deferred-token` が渡された token と
#   一致する時だけ `deferred-rev` を確定する。後続サイクルが deferred 値を上書きすれば token も変わるため、root
#   のサイクルの commit は不一致を検知して未適用 pin を確定しない（次回サイクルで再適用して収束）。
#
# 無更新日の darwin-rebuild 抑止（F3）と home 適用成功の確認:
#   home ステップが実際に switch したかを **CLI の `deferred-rev` marker の有無**で判別し、適用が無い日は
#   darwin-rebuild を起動しない。CLI は適用サイクルごとに `deferred-rev` をクリアし、`--defer-rev-marker` の
#   home が **実際に switch 成功した時だけ** `deferred-rev` を書く（適用済み pin と同一で no-op、または別
#   `dotfiles update` が `update.lock` を保持中で home が skip した場合は書かない）。よって wrapper は home
#   ステップ前に `deferred-rev` を退避・除去し、ステップ後に `deferred-rev` が存在する時だけ darwin と commit へ
#   進む。これにより (a) flake.lock の pin を wrapper で再解析せず（path source の narHash/lastModified pin でも
#   CLI と同一判定になる）、(b) lock 競合で home が skip した日に「pin だけ変わった」ことを誤って適用済みと
#   みなして darwin/commit へ進む drift を防ぐ。commit も同じ `deferred-rev` を確定するため、適用した pin と
#   確定する pin が必ず一致する。
#
# launchd PATH（F1）:
#   launchd daemon の最小 PATH には nix が無い。`dotfiles` は絶対 store パスで起動するが、内部で `nix`・
#   `home-manager`・`darwin-rebuild` を PATH 解決で spawn するため、wrapper で PATH を明示的に通す。sudo は
#   env をリセットするので、ユーザ権限ステップは `env PATH="$PATH"` を前置して PATH を引き継ぐ。
#
# state dir は root で mkdir せず、system.activationScripts で `launchctl asuser` +
# `sudo -u <user>` によりユーザ所有で作成する（launchagents.nix の asuser idiom に倣う）。
#
# state dir の所在（zsh / CLI と一致させる。daemon が書く marker をシェルが消費できるように）:
#   zsh フックと CLI の `state_dir()` は `$XDG_STATE_HOME/dotfiles`（未設定なら `$HOME/.local/state/dotfiles`）を
#   読む。launchd daemon はユーザのログイン環境を持たず `XDG_STATE_HOME` が伝わらないため、wrapper が常に
#   `~/.local/state/dotfiles` を使い、かつ `sudo -u <user>` した CLI へも XDG を渡さないと、利用者が
#   `XDG_STATE_HOME` を設定している場合に daemon（`~/.local/state/dotfiles`）とシェル（`$XDG_STATE_HOME/dotfiles`）が
#   別 marker を読み、`last-applied-rev` / `pending-summary` を消費できず重複適用・要約未表示になる。これを避け、
#   wrapper は **利用者の per-user launchd 環境から `XDG_STATE_HOME` を実行時解決**し（未設定なら HOME 既定へ縮退）、
#   自身の marker 読取りと `sudo -u <user>` CLI 起動の双方で同じ state dir を指す（CLI へは `env XDG_STATE_HOME=...`
#   で伝播し、CLI の `state_dir()` がシェルと同一の dir を解決する）。
{
  user,
  pkgs,
  lib,
  inputs,
  config,
  ...
}:
let
  label = "org.dotfiles.auto-update";
  homeDir = "/Users/${user}";
  # `XDG_STATE_HOME` 未設定時の既定 state base（zsh / CLI の HOME フォールバックと一致）。XDG を設定している
  # 利用者では wrapper が実行時に per-user launchd 環境から `XDG_STATE_HOME` を解決して上書きする。
  defaultStateDir = "${homeDir}/.local/state/dotfiles";
  # 利用者の dotfiles ローカル flake（`dotfiles init` が書く `~/.config/dotfiles`）。root daemon の darwin 適用は
  # root で走るため、`--config-dir` を渡さないと `$HOME`（/var/root）配下の存在しない config を見に行き失敗する。
  # home/darwin 双方にこのユーザ config を明示し、root と user で同じローカル flake を確実に指す。
  configDir = "${homeDir}/.config/dotfiles";

  system = pkgs.stdenv.hostPlatform.system;
  # 絶対 store パスで CLI を指す（PATH 非依存。root daemon の最小環境で確実に解決するため）。
  dotfilesBin = "${inputs.self.packages.${system}.default}/bin/dotfiles";

  # launchd daemon の最小 PATH には nix が無い。`dotfiles` バイナリ自体は絶対 store パスで起動できるが、
  # `dotfiles update home` は内部で `nix`（`nix flake update`）と `home-manager` を、`dotfiles switch darwin` は
  # `darwin-rebuild` を **PATH 解決で** spawn する。これらが見つからないと daemon が失敗するため、wrapper の
  # PATH を明示的に通す。store パスで確実に解決できる `nix`（`config.nix.package`）と coreutils（`mkdir`/`mv`
  # 等の退避処理）を makeBinPath で先頭に置き、nix-darwin/home-manager が提供する `darwin-rebuild`・
  # `home-manager` は実行時 profile（`/run/current-system/sw/bin`・`/etc/profiles/per-user/<user>/bin`・
  # nix default profile）から解決する（これらは build 時の store パスに固定できないため runtime profile で引く）。
  daemonPath = lib.concatStringsSep ":" [
    (lib.makeBinPath [
      config.nix.package
      pkgs.coreutils
    ])
    "/run/current-system/sw/bin"
    "/etc/profiles/per-user/${user}/bin"
    "/nix/var/nix/profiles/default/bin"
    "/usr/bin"
    "/bin"
    "/usr/sbin"
    "/sbin"
  ];

  # 権限分割のみを担う薄いラッパー。業務判断は持たず、誰の権限でどのサブコマンドを呼ぶかだけを決める。
  wrapper = pkgs.writeShellScript "${label}-wrapper" ''
    set -euo pipefail

    # launchd daemon の最小 PATH には nix が無い。wrapper 自身（root の darwin ステップが PATH 解決で
    # `darwin-rebuild` を引く）と、`sudo -u <user> env PATH=... dotfiles ...` 経由で渡すユーザ権限ステップの
    # 双方で nix / home-manager / darwin-rebuild を解決できるよう PATH を確定する。sudo は既定で env を
    # リセットし secure_path で PATH を上書きしうるため、ユーザ権限ステップでは `env PATH="$PATH"` を前置して
    # 明示的に引き継ぐ。
    export PATH=${lib.escapeShellArg daemonPath}

    config_dir=${lib.escapeShellArg configDir}

    # state dir を zsh / CLI と一致させる。launchd daemon はユーザのログイン環境を持たないため、利用者が
    # `XDG_STATE_HOME` を設定していてもここには伝わらない。per-user launchd 環境（`launchctl asuser <uid>
    # launchctl getenv XDG_STATE_HOME`）から利用者の `XDG_STATE_HOME` を実行時解決し、設定されていれば
    # `$XDG_STATE_HOME/dotfiles`、未設定なら HOME 既定（`~/.local/state/dotfiles`）を state dir にする。これにより
    # daemon が書く marker をシェル（CLI の `state_dir()`）が同じ dir で消費でき、重複適用・要約未表示を防ぐ。
    # 解決した `XDG_STATE_HOME` は `sudo -u <user>` する CLI へも `env XDG_STATE_HOME=...` で伝播し、CLI の
    # `state_dir()` が wrapper と同一 dir を解決するようにする。
    uid="$(/usr/bin/id -u ${lib.escapeShellArg user})"
    xdg_state_home="$(/bin/launchctl asuser "$uid" /bin/launchctl getenv XDG_STATE_HOME 2>/dev/null || true)"
    xdg_state_home="$(printf '%s' "$xdg_state_home" | tr -d '[:space:]')"
    if [ -n "$xdg_state_home" ]; then
      state_dir="$xdg_state_home/dotfiles"
    else
      state_dir=${lib.escapeShellArg defaultStateDir}
    fi

    # 解決した XDG を sudo -u する CLI へ伝播するための env 前置を組み立てる（未設定なら付けない）。CLI の
    # `state_dir()` が wrapper と同一 dir を解決し、marker / pending-summary を一致させる。PATH も sudo の
    # env リセットに備えて常に渡す。
    if [ -n "$xdg_state_home" ]; then
      user_env="/usr/bin/env PATH=$PATH XDG_STATE_HOME=$xdg_state_home"
    else
      user_env="/usr/bin/env PATH=$PATH"
    fi

    # ① state dir をユーザ所有で用意（activation でも作るが、daemon 起動時の取りこぼしに備える）。
    #    root では mkdir せず、ユーザ権限で作る（マーカーのユーザ所有保証）。
    /usr/bin/sudo -u ${lib.escapeShellArg user} /bin/mkdir -p "$state_dir"

    # home ステップが実際に switch したかは **CLI の `deferred-rev` marker の有無**で判別する。CLI は適用
    # サイクル冒頭で deferred marker をクリアし、`--defer-rev-marker` の home が実際に switch 成功した時だけ
    # `deferred-rev` を書く（適用済み pin と同一で no-op、または別 `dotfiles update` が `update.lock` を保持中で
    # home が skip した場合は書かない）。判定を確実にするため、home ステップ前にこの marker を退避・除去して
    # おき、ステップ後に存在すれば「この起動の home が実際に適用した」と確定できる。lock 競合で home が skip
    # した場合でも flake.lock の共有 pin だけが進むことはあるが、marker が書かれないので darwin/commit へ
    # 誤って進まない（home 未収束のまま「適用済み」と誤確定する drift を防ぐ）。
    deferred_marker="$state_dir/deferred-rev"
    /usr/bin/sudo -u ${lib.escapeShellArg user} /bin/rm -f "$deferred_marker"

    # この darwin 実行サイクルを識別する **rev マーカー token** を生成する（finding 3368519975）。home defer
    # ステップへ `--rev-marker-token` で渡し、CLI は deferred 値と同じ瞬間にこの token を `deferred-token` へ
    # 控える。同じ token を commit ステップへも渡し、CLI は `deferred-token` がこの token と一致する時だけ
    # `deferred-rev` を確定する。home 適用後に user 側 `update.lock` を解放してから root の darwin を実行する間に
    # 別の login catch-up が新サイクルを始めて `deferred-rev`/`deferred-token` を上書きしても、token 不一致で
    # commit が **root が適用していない後続サイクルの pin を確定しない**（darwin 実行中は lock 解放済みのため
    # 排他では防げない競合を token で防ぐ）。値は衝突しなければよいので pid + epoch 秒で構成する（同一マシン内の
    # 直近サイクルと重複しない単調・一意な識別子）。
    rev_marker_token="$$-$(/bin/date -u +%s)"

    # ② 適用要否判定・home-manager 適用・要約書込みをユーザ権限で実行する。非 tty なので要約は
    #    pending-summary へ書かれる。darwin は別ステップ（root）で適用するため、ここでは home のみを対象に
    #    する（home-manager を root で走らせない）。`--defer-rev-marker` で `last-applied-rev` はまだ書かず、
    #    home+darwin の両成功後（④）に確定する。これにより darwin 失敗時の rev drift を防ぐ。`--config-dir` で
    #    ユーザの `~/.config/dotfiles` を明示し、確実にユーザのローカル flake を指す。sudo は env をリセットし
    #    PATH を secure_path で上書きしうるため、`$user_env`（PATH と解決済み XDG_STATE_HOME を渡す）を前置する。
    #
    #    **lock 競合 skip の専用 exit code を吸収する（finding 3376248532）**: CLI は別の `dotfiles update` が
    #    `update.lock` を保持中で何も適用できなかった場合だけ専用 code（`EX_TEMPFAIL`=75）で終了する。これは想定内の
    #    一時 skip（drift ではない）であり、`set -e` で daemon サイクル全体を異常終了させるべきではない。よって
    #    defer ステップだけ `set -e` を一時停止して exit code を捕捉し、75（lock 競合）は graceful skip、それ以外の
    #    非 0（network/nix 失敗等）は従来どおり異常終了させる（darwin/commit へ進ませない）。exit 0（適用 / no-op）は
    #    続行し、実際に適用したかは下の `deferred-rev` marker の有無で判別する。
    home_defer_rc=0
    set +e
    /usr/bin/sudo -u ${lib.escapeShellArg user} $user_env ${dotfilesBin} update home \
      --config-dir "$config_dir" --defer-rev-marker --rev-marker-token "$rev_marker_token"
    home_defer_rc=$?
    set -e
    if [ "$home_defer_rc" -eq 75 ]; then
      echo "別の dotfiles update が適用中のため home を skip しました（lock 競合・次回再判定）"
      exit 0
    elif [ "$home_defer_rc" -ne 0 ]; then
      echo "home 適用が失敗しました（rc=$home_defer_rc）。darwin/commit を skip します" >&2
      exit "$home_defer_rc"
    fi

    # home が実際に switch したか（=`deferred-rev` marker が書かれたか）で darwin / commit へ進むかを決める。
    # marker が無ければ home は no-op（適用済み pin と同一）か lock 競合 skip であり、darwin-rebuild を起動する
    # 理由が無いので darwin ステップと rev 確定を skip する（無更新日 / 競合日に重い system switch を走らせない）。
    # marker の有無判定は CLI の pin identity（rev→narHash→lastModified）に従うため、path source（rev-less）でも
    # github source でも CLI と同一の適用判定になる（wrapper では flake.lock を再解析しない）。
    if ! /usr/bin/sudo -u ${lib.escapeShellArg user} /bin/test -f "$deferred_marker"; then
      echo "home に更新がないため darwin-rebuild を skip します"
      exit 0
    fi

    # ③ darwin 部分は root のまま、sudo を前置しない経路で適用する（既に root daemon のため）。root では
    #    `$HOME` が /var/root のため `--config-dir` でユーザ config を明示しないと存在しない config を見に行く。
    #    PATH は wrapper の export 済み値を継承し、`darwin-rebuild` を runtime profile から解決する。
    DOTFILES_DARWIN_REBUILD_SUDO=0 ${dotfilesBin} switch darwin \
      --config-dir "$config_dir" --no-sudo

    # ④ home+darwin の両適用が成功した後にだけ rev マーカーを確定する。set -e により②③のいずれかが失敗
    #    すると④へ到達せず、`last-applied-rev` は前回値のまま残る（次回再適用で収束させ、drift を回避）。
    #    commit は ② が書いた `deferred-rev`（実際に適用した pin）を読んで確定するため、適用した pin と確定する
    #    pin が必ず一致する。マーカーはユーザ所有で書くため、確定もユーザ権限で行う。`--config-dir` で読む pin
    #    を②と一致させ、`$user_env` で同一 state dir（XDG）を CLI へ伝える。
    #
    #    commit でも lock 競合 skip（専用 code 75）は想定内の一時 skip として graceful に扱う（次サイクルで再確定）。
    #    rev 未確定でも darwin は適用済みであり、次サイクルが冪等再適用 + 再確定で収束する。それ以外の非 0
    #    （要約失敗は CLI 内で best-effort 化され exit 0 のため該当しない。pin 解析失敗等の異常）は set -e で異常終了。
    commit_rc=0
    set +e
    /usr/bin/sudo -u ${lib.escapeShellArg user} $user_env ${dotfilesBin} update home \
      --config-dir "$config_dir" --commit-rev-marker --rev-marker-token "$rev_marker_token"
    commit_rc=$?
    set -e
    if [ "$commit_rc" -eq 75 ]; then
      echo "rev 確定が lock 競合で skip されました（次サイクルで再確定・darwin は適用済み）"
      exit 0
    elif [ "$commit_rc" -ne 0 ]; then
      echo "rev 確定が失敗しました（rc=$commit_rc）" >&2
      exit "$commit_rc"
    fi
  '';
in
{
  launchd.daemons.${label} = {
    serviceConfig = {
      Label = label;
      # ProgramArguments は権限分割ラッパーそのもの（薄い層）。業務判断は dotfiles CLI 側に置く。
      ProgramArguments = [ "${wrapper}" ];
      # nightly bump（CI の自動マージ）後に収束させる時刻。ログイン前提を持たないので毎日 09:00。
      StartCalendarInterval = [
        {
          Hour = 9;
          Minute = 0;
        }
      ];
      # 起動時の即時実行はしない（ブート直後の無人適用を避け、定期発火のみで動かす）。
      RunAtLoad = false;
      StandardOutPath = "/var/log/${label}.out.log";
      StandardErrorPath = "/var/log/${label}.err.log";
    };
  };

  # state dir をユーザ所有で作成する。root で mkdir すると root 所有になり、daemon の `sudo -u <user>` 書込みや
  # シェルからの consume が壊れるため、launchctl asuser + sudo -u でユーザ権限で作る。所在は wrapper / CLI と
  # 一致させ、per-user launchd 環境の `XDG_STATE_HOME` を尊重する（設定済みなら `$XDG_STATE_HOME/dotfiles`、
  # 未設定なら `~/.local/state/dotfiles`）。wrapper も runtime で同じ解決をして作り直すため、ここは先行作成の
  # best-effort であり、解決失敗時は既定 dir へ縮退する。
  system.activationScripts.postActivation.text = lib.mkAfter ''
    uid="$(id -u ${lib.escapeShellArg user})"
    xdg_state_home="$(launchctl asuser "$uid" launchctl getenv XDG_STATE_HOME 2>/dev/null || true)"
    xdg_state_home="$(printf '%s' "$xdg_state_home" | tr -d '[:space:]')"
    if [ -n "$xdg_state_home" ]; then
      state_dir="$xdg_state_home/dotfiles"
    else
      state_dir=${lib.escapeShellArg defaultStateDir}
    fi
    launchctl asuser "$uid" sudo --user=${lib.escapeShellArg user} -- mkdir -p "$state_dir"
  '';
}

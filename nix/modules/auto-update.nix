# nightly bump 後に各マシンを repo pin へ無人で収束させる auto-update daemon（単純版）。
#
# launchd daemon（root/system）が `StartCalendarInterval` で定期起動し、**root のまま** `dotfiles update` を
# 1 回呼ぶだけにする。適用要否判定・home-manager 適用・要約/marker 書込みといった業務判断は **すべて
# `dotfiles update` CLI 側** にあり、nix へは漏らさない。
#
# 単純版で撤去したもの（過剰設計）:
#   - home/darwin を別ステップで適用する権限分割の二段適用（`--defer-rev-marker` / `--commit-rev-marker`）。
#   - サイクル識別 token（`rev_marker_token`）と `deferred-rev` / `deferred-token` marker による drift 防止機械。
#   - lock 競合の専用 exit code（EX_TEMPFAIL=75）の graceful 吸収。
#   `dotfiles update` は単一 marker `last-applied-rev` で冪等（同一 pin なら no-op）であり、同時更新者を想定しない
#   （万一の同時実行は nix 自身の store/profile ロックと冪等性に委ねる）。よって排他・scope・defer は不要。
#
# 権限（root 実行・apply 分割なし・state はユーザ所有へ chown）:
#   - home-manager は nix-darwin モジュール（`darwin.nix` の `home-manager.users.${user}`）として組み込まれ、
#     **root の `darwin-rebuild switch` が system と home-manager の両方を一度に適用する**。したがって daemon は
#     root のまま `dotfiles update --no-sudo` を 1 回呼べば全部適用できる（`sudo -u <user>` で権限を落とすと
#     darwin-rebuild が root を得られず失敗するため、落とさない）。`--no-sudo` /
#     `DOTFILES_DARWIN_REBUILD_SUDO=0` は **root で darwin-rebuild に sudo を前置しない**正しい指定で維持する。
#   - apply は分割しない。書いた state の所有権だけを後から調整する: root 実行で生まれる
#     `last-applied-rev` / `pending-summary` 等は、適用後に `chown -R <user>` で **ユーザ所有に直す**。
#     これは zsh の show-once（ユーザ権限）が `pending-summary` を rename で消費するために必須である。
#
# state dir はユーザ所有の `~/.local/state/dotfiles`。利用者が `XDG_STATE_HOME` を設定していればそれを尊重し
# （値はそのまま使い空白も保持する）、CLI の `state_dir()` と同一の dir を指すよう wrapper が `HOME`（と
# 設定時 `XDG_STATE_HOME`）を CLI へ渡して state_dir をユーザの dir に向ける。root 実行で書いた state は
# 適用後に `chown -R <user>` でユーザ所有へ直す。これにより daemon が書く marker をシェル（CLI の
# `state_dir()`）が同じ dir・同じ所有権で消費でき、重複適用・要約未表示を防ぐ。
#
# launchd PATH: 最小 PATH には nix が無い。`dotfiles` は絶対 store パスで起動するが、内部で `nix` /
# `home-manager` / `darwin-rebuild` を PATH 解決で spawn するため wrapper で PATH を通す。
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
  configDir = "${homeDir}/.config/dotfiles";

  system = pkgs.stdenv.hostPlatform.system;
  # x86_64-darwin 等、flake の `packageSystems` に含まれない system では `packages.${system}` が存在しない。
  # その system で評価が落ちないよう、package が無ければ daemon を定義しない（degrade）。
  hasPackage = inputs.self.packages ? ${system};

  # 絶対 store パスで CLI を指す（PATH 非依存。root daemon の最小環境で確実に解決するため）。
  dotfilesBin = "${inputs.self.packages.${system}.default}/bin/dotfiles";

  # makeBinPath で確実に解決できる `nix` / coreutils を先頭に置き、`darwin-rebuild` / `home-manager` は実行時
  # profile から引く（build 時の store パスに固定できないため runtime profile で解決する）。
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

  # launchd timer が呼ぶ薄い wrapper。**root のまま** `dotfiles update --no-sudo` を 1 回呼ぶだけ。
  # darwin-rebuild は root の `darwin-rebuild switch` で system と home-manager の両方を一度に適用する
  # （`DOTFILES_DARWIN_REBUILD_SUDO=0` で root が darwin-rebuild に sudo を前置しない正しい経路）。
  # 適用後、root 実行で生まれた state を `chown -R <user>` でユーザ所有へ直し、zsh show-once が消費できるようにする。
  wrapper = pkgs.writeShellScript "${label}-wrapper" ''
    set -euo pipefail

    export PATH=${lib.escapeShellArg daemonPath}

    # state dir を CLI / シェルと一致させる。launchd daemon はユーザのログイン環境を持たないため、wrapper が
    # ユーザの `HOME`（と利用者設定時 `XDG_STATE_HOME`）を CLI へ渡して CLI の `state_dir()` をユーザの dir へ
    # 向ける（marker / pending-summary を一致させる）。`XDG_STATE_HOME` は per-user launchd 環境から実行時解決し、
    # CLI の解決規則と同じく **そのまま** 使う（末尾改行だけ除去し空白を含む正当なパスは保持する）。
    uid="$(/usr/bin/id -u ${lib.escapeShellArg user})"
    xdg_state_home="$(/bin/launchctl asuser "$uid" /bin/launchctl getenv XDG_STATE_HOME 2>/dev/null || true)"
    # `launchctl getenv` は末尾に改行を付けるため、末尾改行だけを 1 つ落とす（中身の空白は保持する）。
    xdg_state_home="''${xdg_state_home%$'\n'}"

    # root のまま実行する。`--no-sudo` / `DOTFILES_DARWIN_REBUILD_SUDO=0` は root で darwin-rebuild に sudo を
    # 前置しない正しい指定。`HOME` をユーザの home に向け、CLI の state_dir() がユーザ dir を指すようにする。
    if [ -n "$xdg_state_home" ]; then
      /usr/bin/env PATH="$PATH" HOME=${lib.escapeShellArg homeDir} XDG_STATE_HOME="$xdg_state_home" \
        DOTFILES_DARWIN_REBUILD_SUDO=0 \
        ${dotfilesBin} update --config-dir ${lib.escapeShellArg configDir} --no-sudo
      # root 実行で書いた state（XDG_STATE_HOME 配下の dotfiles dir）をユーザ所有へ直す。
      state_dir="$xdg_state_home/dotfiles"
    else
      /usr/bin/env PATH="$PATH" HOME=${lib.escapeShellArg homeDir} \
        DOTFILES_DARWIN_REBUILD_SUDO=0 \
        ${dotfilesBin} update --config-dir ${lib.escapeShellArg configDir} --no-sudo
      # XDG 未設定なら CLI と同じ既定（$HOME/.local/state/dotfiles）。
      state_dir=${lib.escapeShellArg homeDir}"/.local/state/dotfiles"
    fi

    # zsh show-once（ユーザ権限）が `pending-summary` を rename 消費できるよう、root が書いた state を
    # ユーザ所有へ直す（apply は分割せず、書いた state の所有権だけ調整する）。dir 不在なら no-op。
    if [ -d "$state_dir" ]; then
      /usr/sbin/chown -R ${lib.escapeShellArg user} "$state_dir"
    fi
  '';
in
{
  # x86_64-darwin 等 `packageSystems` 外の system では `packages.${system}` が無いため daemon を定義しない。
  # 条件はモジュールの **構造**（宣言する option 集合）ではなく option の **値** に `mkIf` で寄せる。構造を
  # `inputs` 由来の条件で変えると `config` 解決と循環して infinite recursion になる。`mkIf false` は遅延評価
  # のため、package が無い system では内側の `wrapper`（=`dotfilesBin` の force）も評価されない。
  launchd.daemons.${label} = lib.mkIf hasPackage {
    serviceConfig = {
      Label = label;
      ProgramArguments = [ "${wrapper}" ];
      # nightly bump（CI の自動マージ）後に収束させる時刻。ログイン前提を持たないので毎日 09:00。
      StartCalendarInterval = [
        {
          Hour = 9;
          Minute = 0;
        }
      ];
      # ブート直後の無人適用を避け、定期発火のみで動かす。
      RunAtLoad = false;
      StandardOutPath = "/var/log/${label}.out.log";
      StandardErrorPath = "/var/log/${label}.err.log";
    };
  };
}

# nightly bump 後に各マシンを repo pin へ無人で収束させる auto-update daemon（単純版）。
#
# launchd daemon（root/system）が `StartCalendarInterval` で定期起動し、`sudo -u <user> dotfiles update` を
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
# 権限:
#   - home-manager 適用とユーザ状態（`last-applied-rev` / `pending-summary` 等）の書込みは `sudo -u <user>` で
#     ユーザ権限で行う（home-manager を root で走らせない・marker をユーザ所有で書く）。
#   - darwin-rebuild は既に root のため `DOTFILES_DARWIN_REBUILD_SUDO=0` で sudo を前置しない経路を使う
#     （`dotfiles update` が内部の switch darwin でこの env を尊重する）。
#
# state dir はユーザ所有の `~/.local/state/dotfiles`。利用者が `XDG_STATE_HOME` を設定していればそれを尊重し
# （値はそのまま使い空白も保持する）、CLI の `state_dir()` と同一の dir を指すよう `sudo -u <user>` する CLI へ
# `XDG_STATE_HOME` を伝播する。これにより daemon が書く marker をシェル（CLI の `state_dir()`）が同じ dir で
# 消費でき、重複適用・要約未表示を防ぐ。
#
# launchd PATH: 最小 PATH には nix が無い。`dotfiles` は絶対 store パスで起動するが、内部で `nix` /
# `home-manager` / `darwin-rebuild` を PATH 解決で spawn するため wrapper で PATH を通す。sudo は env を
# リセットするので、ユーザ権限経路は `env PATH=... [XDG_STATE_HOME=...]` を前置して引き継ぐ。
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

  # launchd timer が呼ぶ薄い wrapper。`sudo -u <user> dotfiles update` を 1 回呼ぶだけ。darwin は CLI 内部で
  # `DOTFILES_DARWIN_REBUILD_SUDO=0` の sudo 無し経路を使う。
  wrapper = pkgs.writeShellScript "${label}-wrapper" ''
    set -euo pipefail

    export PATH=${lib.escapeShellArg daemonPath}

    # state dir を CLI / シェルと一致させる。launchd daemon はユーザのログイン環境を持たないため、利用者の
    # `XDG_STATE_HOME` を per-user launchd 環境から実行時解決し、設定されていれば同値を `sudo -u <user>` する
    # CLI へ伝播して CLI の `state_dir()` が同一 dir を指すようにする（marker / pending-summary を一致させる）。
    # 値は CLI の解決規則と同じく **そのまま** 使う（trim せず空白も保持する）。
    uid="$(/usr/bin/id -u ${lib.escapeShellArg user})"
    xdg_state_home="$(/bin/launchctl asuser "$uid" /bin/launchctl getenv XDG_STATE_HOME 2>/dev/null || true)"

    # darwin は既に root のため sudo を前置しない経路を使う。home-manager とユーザ marker はユーザ権限で
    # 書くため `sudo -u <user>` する。sudo は env をリセットするので PATH（と解決できた XDG）を明示的に渡す。
    if [ -n "$xdg_state_home" ]; then
      /usr/bin/sudo -u ${lib.escapeShellArg user} \
        /usr/bin/env PATH="$PATH" XDG_STATE_HOME="$xdg_state_home" DOTFILES_DARWIN_REBUILD_SUDO=0 \
        ${dotfilesBin} update --config-dir ${lib.escapeShellArg configDir} --no-sudo
    else
      /usr/bin/sudo -u ${lib.escapeShellArg user} \
        /usr/bin/env PATH="$PATH" DOTFILES_DARWIN_REBUILD_SUDO=0 \
        ${dotfilesBin} update --config-dir ${lib.escapeShellArg configDir} --no-sudo
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

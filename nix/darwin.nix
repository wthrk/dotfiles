# nix-darwin で管理するホスト設定の最上位モジュール。
#
# 引数 `user` は `system.primaryUser`、Home Manager の対象ユーザー、nix-homebrew の所有者に使う。
# 引数 `host` は `networking.hostName` に使う。`root` は設定ファイルのリンク元、`inputs` と
# `homebrewTaps` は Home Manager / nix-homebrew 連携と pin 済み tap の生成に使う。
# 評価結果は Nix 設定、macOS defaults、Homebrew、launch agent、Home Manager をまとめた
# `darwinConfigurations.<host>` 用の構成になる。
{
  homebrewTaps,
  inputs,
  lib,
  root,
  user,
  host,
  pkgs,
  config,
  ...
}:
let
  # auto-update daemon は nightly bump 後に各マシンを repo pin へ無人収束させる薄い launchd service である。
  # launchd timer は 09:00 に 1 回だけ起動し、root daemon から `dotfiles update` の既定 target（all）を呼ぶ。
  # CLI 側が lock 更新と Home Manager を対象ユーザー権限へ降格し、darwin-rebuild だけを root のまま適用する。
  # wrapper は対象ユーザー・HOME・config-dir・host を明示し、適用順序や target semantics は CLI の `update` / `switch`
  # と同じ実装に委ねる。
  autoUpdateLabel = "org.dotfiles.auto-update";
  homeDir = "/Users/${user}";
  configDir = "${homeDir}/.config/dotfiles";

  autoUpdateSystem = pkgs.stdenv.hostPlatform.system;
  # x86_64-darwin 等、flake の `packageSystems` に含まれない system では `packages.${system}` が存在しない。その
  # system で評価が落ちないよう、package が無ければ daemon を定義しない（degrade）。動的キー（`system` は変数）の
  # 存在判定は `builtins.hasAttr` で行う。
  hasAutoUpdatePackage = builtins.hasAttr autoUpdateSystem inputs.self.packages;

  # 絶対 store パスで CLI を指す（PATH 非依存。launchd の最小環境で確実に解決するため）。
  dotfilesBin = "${inputs.self.packages.${autoUpdateSystem}.default}/bin/dotfiles";

  # makeBinPath で確実に解決できる `nix` / coreutils を先頭に置き、`darwin-rebuild` / `home-manager` は実行時
  # profile から引く（build 時の store パスに固定できないため runtime profile で解決する）。CLI に焼き込まれた
  # `DOTFILES_DARWIN_REBUILD` / `DOTFILES_HOME_MANAGER` 絶対パスが効くため、この PATH は `nix` の解決にだけ使う。
  autoUpdatePath = lib.concatStringsSep ":" [
    (lib.makeBinPath [
      config.nix.package
      pkgs.coreutils
    ])
    "/etc/profiles/per-user/${user}/bin"
    "${homeDir}/.nix-profile/bin"
    "/run/current-system/sw/bin"
    "/nix/var/nix/profiles/default/bin"
    "/usr/bin"
    "/bin"
    "/usr/sbin"
    "/sbin"
  ];

  # launchd timer が呼ぶ薄い wrapper。target は省略し、手動 `dotfiles update` と同じ既定 `all`
  # （lock 更新後に Home Manager、続いて nix-darwin）を使う。root daemon から呼ぶため `--user` を明示し、CLI が
  # lock 更新と standalone Home Manager を `sudo -H -u ${user}` で実行できるようにする。`HOME` と `--config-dir` は
  # 対象ユーザーのローカル flake へ固定し、root の `$HOME/.config/dotfiles` や `homeConfigurations.root` を参照しない。
  # `--host` は `dotfiles init --host` で短縮 hostname と異なる出力名を使った環境でも `#<host>` を正しく参照する。
  autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
    set -euo pipefail

    export PATH=${lib.escapeShellArg autoUpdatePath}

    exec env HOME=${lib.escapeShellArg homeDir} ${dotfilesBin} update \
      --config-dir ${lib.escapeShellArg configDir} \
      --user ${lib.escapeShellArg user} \
      --host ${lib.escapeShellArg host}
  '';
in
{
  imports = [
    ./modules/macos.nix
    ./modules/homebrew.nix
    ./modules/macos-defaults.nix
    ./modules/launchagents.nix
  ];

  # x86_64-darwin 等 `packageSystems` 外の system では `packages.${system}` が無いため daemon を定義しない。条件は
  # モジュールの **構造**（宣言する option 集合）ではなく option の **値** に `mkIf` で寄せる。`mkIf false` は遅延
  # 評価のため、package が無い system では内側の `autoUpdateWrapper`（=`dotfilesBin` の force）も評価されない。
  launchd.daemons.${autoUpdateLabel} = lib.mkIf hasAutoUpdatePackage {
    serviceConfig = {
      Label = autoUpdateLabel;
      ProgramArguments = [ "${autoUpdateWrapper}" ];
      # nightly bump（CI の自動マージ）後に収束させる時刻。ログイン前提を持たないので毎日 09:00。
      StartCalendarInterval = [
        {
          Hour = 9;
          Minute = 0;
        }
      ];
      # ブート直後の無人適用を避け、定期発火のみで動かす。
      RunAtLoad = false;
      StandardOutPath = "/var/log/${autoUpdateLabel}.out.log";
      StandardErrorPath = "/var/log/${autoUpdateLabel}.err.log";
    };
  };

  nix.settings = {
    experimental-features = [
      "nix-command"
      "flakes"
    ];
    trusted-users = [
      "root"
      user
    ];
  };

  nixpkgs.config.allowUnfree = true;

  networking.hostName = lib.mkDefault host;

  programs.zsh.enable = true;

  # sudo の PAM 設定を nix-darwin で管理し、Touch ID 認証を通常端末と tmux/screen の両方で使えるようにする。
  security.pam.services.sudo_local = {
    touchIdAuth = true;
    reattach = true;
  };

  users.users.${user} = {
    home = "/Users/${user}";
    shell = pkgs.zsh;
  };

  system.primaryUser = user;

  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.backupFileExtension = "before-home-manager";
  home-manager.extraSpecialArgs = { inherit inputs root user; };
  home-manager.users.${user} = import ./home.nix;

  nix-homebrew = {
    enable = true;
    inherit user;
    autoMigrate = true;
    mutableTaps = false;
    taps = builtins.listToAttrs (
      map (tap: {
        name = tap.nixHomebrewName;
        value = tap.source;
      }) homebrewTaps
    );
  };

  system.stateVersion = 6;
}

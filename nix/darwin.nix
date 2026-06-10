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
  # launchd timer は 09:00 に 1 回だけ起動し、`sudo -u <user> dotfiles update` を **ユーザ権限**で呼ぶ。flake.lock
  # 更新（`nix flake update`）はユーザ権限で走り flake.lock はユーザ所有のまま更新されるため、所有権の戻し処理を
  # 持たない。system 適用（`darwin-rebuild switch`）は CLI 内の `sudo darwin-rebuild` が昇格して行い、
  # 無人実行を成立させるため当該ユーザへ `darwin-rebuild switch` の NOPASSWD sudo を最小スコープで付与する。適用要否
  # 判定・home-manager/darwin 適用といった業務判断はすべて `dotfiles update` CLI 側にあり、wrapper は PATH を通して
  # CLI を呼ぶだけの薄い層である。
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
  # `DOTFILES_DARWIN_REBUILD` / `DOTFILES_HOME_MANAGER` 絶対パスが効くため、この PATH は `nix` / `sudo` の解決にだけ
  # 使う。
  autoUpdatePath = lib.concatStringsSep ":" [
    (lib.makeBinPath [
      config.nix.package
      pkgs.coreutils
    ])
    "/run/current-system/sw/bin"
    "/nix/var/nix/profiles/default/bin"
    "/usr/bin"
    "/bin"
    "/usr/sbin"
    "/sbin"
  ];

  # launchd timer が呼ぶ薄い wrapper。`sudo -u <user> dotfiles update` を 1 回 exec するだけ。`--config-dir` /
  # `--host` は `dotfiles init --host` で短縮 hostname と異なる出力名を使った環境でも switch_darwin が正しい
  # `#<host>` を参照するよう渡す（無指定だと daemon が存在しない `#<current-host>` を引いて失敗しうる）。wrapper は
  # `--config-dir` を明示で渡すため、CLI の config_dir 解決は HOME に依存せずユーザ dir を指す。
  autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
    set -euo pipefail

    export PATH=${lib.escapeShellArg autoUpdatePath}

    exec sudo -u ${lib.escapeShellArg user} ${dotfilesBin} update \
      --config-dir ${lib.escapeShellArg configDir} --host ${lib.escapeShellArg host}
  '';

  # 無人 update を成立させるため、`dotfiles update` がユーザ権限で実行する CLI 内の `sudo darwin-rebuild switch` に
  # だけ NOPASSWD を最小付与する。darwin-rebuild の store パスは build ごとに変わるためワイルドカードで指し、許可は
  # `switch` サブコマンドに限定する（広い `darwin-rebuild *` や任意コマンドの sudo は与えない）。
  autoUpdateSudoers = ''
    ${user} ALL=(root) NOPASSWD: /nix/store/*-darwin-rebuild/bin/darwin-rebuild switch *
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

  # auto-update daemon が存在する system でだけ sudoers 断片を置く。`sudo` は誤った権限の sudoers ファイルを無視
  # するため、`environment.etc` の `mode` を `0440` に固定する。
  environment.etc."sudoers.d/${autoUpdateLabel}" = lib.mkIf hasAutoUpdatePackage {
    text = autoUpdateSudoers;
    mode = "0440";
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

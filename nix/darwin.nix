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
  # launchd timer は 09:00 に 1 回だけ起動し、`dotfiles update` を root のまま呼ぶ。root daemon が直接実行するため
  # sudo を要さず、`darwin-rebuild switch` も CLI 内で root のまま sudo なしで適用する。flake.lock 更新
  # （`nix flake update`）も root で走るため所有権の juggling を持たず、sudoers / NOPASSWD / chown も持たない。適用
  # 要否判定・home-manager/darwin 適用といった業務判断はすべて `dotfiles update` CLI 側にあり、wrapper は PATH を
  # 通して CLI を呼ぶだけの薄い層である。
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
    "/run/current-system/sw/bin"
    "/nix/var/nix/profiles/default/bin"
    "/usr/bin"
    "/bin"
    "/usr/sbin"
    "/sbin"
  ];

  # launchd timer が呼ぶ薄い wrapper。`dotfiles update` を root のまま 1 回 exec するだけ。target を `darwin` に
  # 固定するため、`darwin-rebuild switch --flake <config>#<host>` だけを適用する。この nix-darwin 構成は
  # `home-manager.darwinModules.home-manager` を取り込み `home-manager.users.${user}` を宣言するため、`darwin-rebuild
  # switch` が system と Home Manager を一括適用する。target 無指定（既定 `all`）だと standalone の
  # `home-manager switch --flake <config>#<id -un>` を先に実行し、root daemon では `id -un` が root を返すうえ生成
  # flake に `homeConfigurations.root` が無いため失敗し、後続の darwin 適用に到達しない。`HOME` を対象ユーザの home に
  # 固定し、`--config-dir` / `--host` を明示で渡すため、CLI の config_dir 解決は環境に依存せずユーザ dir を指し、
  # `--host` は `dotfiles init --host` で短縮 hostname と異なる出力名を使った環境でも `#<host>` を正しく参照する。
  autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
    set -euo pipefail

    export PATH=${lib.escapeShellArg autoUpdatePath}

    exec env HOME=${lib.escapeShellArg homeDir} ${dotfilesBin} update darwin \
      --config-dir ${lib.escapeShellArg configDir} --host ${lib.escapeShellArg host}
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

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
  # **root のまま** `dotfiles update --config-dir <homeDir>/.config/dotfiles --no-sudo` を 09:00 に 1 回呼ぶ。適用
  # 要否判定・home-manager/darwin 適用・要約/marker 書込みといった業務判断は **すべて `dotfiles update` CLI 側**に
  # あり、wrapper は PATH を通して CLI を呼び、root 書込み分の所有権をユーザへ戻すだけの薄い層である。
  #
  # root 実行の理由: home-manager は nix-darwin モジュールとして組み込まれ、root の `darwin-rebuild switch` が
  # system と home-manager を一度に適用する。`sudo -u <user>` で権限を落とすと darwin-rebuild が root を得られず
  # 失敗するため落とさない。`--no-sudo` / `DOTFILES_DARWIN_REBUILD_SUDO=0` は root で darwin-rebuild に sudo を
  # 前置しない正しい指定。
  autoUpdateLabel = "org.dotfiles.auto-update";
  homeDir = "/Users/${user}";
  configDir = "${homeDir}/.config/dotfiles";

  autoUpdateSystem = pkgs.stdenv.hostPlatform.system;
  # x86_64-darwin 等、flake の `packageSystems` に含まれない system では `packages.${system}` が存在しない。その
  # system で評価が落ちないよう、package が無ければ daemon を定義しない（degrade）。動的キー（`system` は変数）の
  # 存在判定は `builtins.hasAttr` で行う。
  hasAutoUpdatePackage = builtins.hasAttr autoUpdateSystem inputs.self.packages;

  # 絶対 store パスで CLI を指す（PATH 非依存。root daemon の最小環境で確実に解決するため）。
  dotfilesBin = "${inputs.self.packages.${autoUpdateSystem}.default}/bin/dotfiles";

  # makeBinPath で確実に解決できる `nix` / coreutils を先頭に置き、`darwin-rebuild` / `home-manager` は実行時
  # profile から引く（build 時の store パスに固定できないため runtime profile で解決する）。
  autoUpdatePath = lib.concatStringsSep ":" [
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

  # launchd timer が呼ぶ薄い wrapper。**root のまま** `dotfiles update --no-sudo` を 1 回 exec するだけ。PATH と
  # `HOME` / `DOTFILES_DARWIN_REBUILD_SUDO=0` を渡して CLI へ委ねる。
  autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
    set -euo pipefail

    export PATH=${lib.escapeShellArg autoUpdatePath}

    # root 実行で root 所有になった state dir / flake.lock をユーザへ戻す。zsh の pending-summary show-once は
    # ユーザ権限の rename で消費するため、root 書込み分を戻さないと次回ユーザ実行が EACCES になる。`set -e` で
    # update 失敗時もスクリプト終了時に必ず所有権を戻すよう、chown は EXIT trap に置く（成功・失敗どちらでも実行）。
    #
    # symlink を辿らず所有権を戻す。`chown` は既定で symlink を辿るため、ユーザ（または侵害されたユーザ
    # セッション）が flake.lock や state dir 配下を任意パスへの symlink にすり替えると、root がその指す先の
    # 所有権を変えうる。macOS(BSD) の `chown` は `-h`（symlink 自体を対象にし、辿らない）と `-R`（再帰）を
    # 受ける。再帰の state dir は `-hR`、単一 flake.lock は `-h` を付け、symlink を辿らないことを明示する。
    # best-effort（存在しなくても落とさない）。
    trap '/usr/sbin/chown -hR ${lib.escapeShellArg user} ${lib.escapeShellArg "${homeDir}/.local/state/dotfiles"} 2>/dev/null || true; /usr/sbin/chown -h ${lib.escapeShellArg user} ${lib.escapeShellArg "${configDir}/flake.lock"} 2>/dev/null || true' EXIT

    # root のまま実行する。`HOME` をユーザの home に向け、CLI の state_dir() / config_dir がユーザ dir を指す
    # ようにする。`--host` は `dotfiles init --host` で短縮 hostname と異なる出力名を使った環境でも switch_darwin が
    # 正しい `#<host>` を参照するよう渡す（無指定だと daemon が存在しない `#<current-host>` を引いて失敗しうる）。
    env HOME=${lib.escapeShellArg homeDir} DOTFILES_DARWIN_REBUILD_SUDO=0 \
      ${dotfilesBin} update --config-dir ${lib.escapeShellArg configDir} --host ${lib.escapeShellArg host} --no-sudo
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

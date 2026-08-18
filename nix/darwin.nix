# nix-darwin で管理するホスト設定の最上位モジュール。
#
# 引数 `user` は `system.primaryUser`、Home Manager の対象ユーザー、nix-homebrew の所有者に使う。
# 引数 `host` は `networking.hostName` に使う。`includeSelfPackage` は Home Manager 側へそのまま渡し、
# `home.packages` へ repo 自身の `dotfiles` CLI を含めるかを制御する。`root` は設定ファイルのリンク元、
# `inputs` と `homebrewTaps` は Home Manager / nix-homebrew 連携と pin 済み tap の生成に使う。
# 評価結果は Nix 設定、macOS defaults、Homebrew、launchd daemon、Home Manager をまとめた
# `darwinConfigurations.<host>` 用の構成になる。
{
  homebrewTaps,
  inputs,
  lib,
  root,
  user,
  host,
  includeSelfPackage ? true,
  pkgs,
  config,
  ...
}:
let
  # auto-update daemon は nightly bump 後に各マシンを repo pin へ無人収束させる薄い launchd service である。
  # launchd timer は 09:00 に 1 回だけ起動し、root daemon から `dotfiles update` を呼ぶ。対象ユーザーは
  # 指定せず、このマシンでローカル flake を持つ全ユーザーの解決を CLI に委ねる。CLI 側が各ユーザーの
  # lock 更新と Home Manager をそのユーザー権限へ降格し、system 層は所有者の flake からだけ適用する。
  # マシンの自動更新はこの 1 個の daemon であり、スケジュールもログも 1 箇所に残る。
  autoUpdateLabel = "org.dotfiles.auto-update";

  homeDir = "/Users/${user}";

  # 退役した CapsLock→Ctrl 実装（login のたびに `hidutil` で HID mapping を掛け直していた user agent）。
  retiredKeyboardRemapLabel = "org.dotfiles.keyboard-remap";
  retiredKeyboardRemapPlist = "${homeDir}/Library/LaunchAgents/${retiredKeyboardRemapLabel}.plist";

  autoUpdateSystem = pkgs.stdenv.hostPlatform.system;
  # x86_64-darwin 等、flake の `packageSystems` に含まれない system では `packages.${system}` が存在しない。その
  # system で評価が落ちないよう、package が無ければ daemon を定義しない（degrade）。動的キー（`system` は変数）の
  # 存在判定は `builtins.hasAttr` で行う。
  hasAutoUpdatePackage = builtins.hasAttr autoUpdateSystem inputs.self.packages;

  # 絶対 store パスで CLI を指す（PATH 非依存。launchd の最小環境で確実に解決するため）。
  dotfilesBin = "${inputs.self.packages.${autoUpdateSystem}.default}/bin/dotfiles";

  # launchd の最小環境で、CLI が絶対パスを持たないコマンドを解決するための PATH。CLI 自身が引くのは
  # `sudo`（対象ユーザーへの降格）と `dscl`（ローカル flake を持つユーザーの列挙）で、`nix` / coreutils は
  # そこから起動する子プロセスが使う。`darwin-rebuild` / `home-manager` は `flake.nix` の `wrapProgram` が
  # `DOTFILES_DARWIN_REBUILD` / `DOTFILES_HOME_MANAGER` へ焼き込んだ絶対パスで解決するため、ここには要らない。
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

  # launchd timer が呼ぶ薄い wrapper。target と対象ユーザーは指定しない。root で `--user` 省略の
  # `dotfiles update` は、このマシンでローカル flake を持つ全ユーザーを対象にし、各ユーザーの
  # 適用範囲もそのユーザーの scope から決まる。ここへ利用者名や config-dir を埋め込むと、daemon が
  # 1 人だけを見る状態に戻る。`--host` はマシン単位の値で、`dotfiles init --host` で短縮 hostname と
  # 異なる出力名を使った環境でも `#<host>` を正しく参照する。
  autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
    set -euo pipefail

    export PATH=${lib.escapeShellArg autoUpdatePath}

    exec ${dotfilesBin} update --host ${lib.escapeShellArg host}
  '';

  # 実行者以外が console を持っている状況を判定し、その間だけ pam_tid.so を認証経路から外す PAM module。
  pamTouchIdSessionGuard = pkgs.callPackage ./pam-touchid-session-guard { };
in
{
  imports = [
    ./modules/macos.nix
    ./modules/homebrew.nix
    ./modules/macos-defaults.nix
  ];

  # nix-darwin 側の launchd 宣言はこの daemon だけに保ち、`launchd.user.agents` は空にする。user agent が
  # 0 件だと上流の `userLaunchd`（`modules/system/launchd.nix`）が旧世代の user agent を撤去するループごと
  # 生成されないため、一度足した user agent は後で宣言を消しても適用済みマシンに残る。
  #
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

  # 旧 CapsLock→Ctrl 実装の退役。宣言を消すだけでは適用済みマシンから消えないため、activation で撤去する。
  # nix-darwin の `launchd.user.agents` で足した user agent は上流の退役ループが走らないので残り、`hidutil` の
  # `UserKeyMapping` は `system.keyboard.enableKeyMapping` を落としてもクリアされない。旧 plist が残っている
  # マシンでだけ走らせ、撤去済みのマシンで `UserKeyMapping` を触り続けないようにする。旧 agent が残る実マシンが
  # 無くなったらこの宣言ごと削除してよい。
  system.activationScripts.postActivation.text = lib.mkAfter ''
    uid="$(id -u ${lib.escapeShellArg user})"

    if [ -e ${lib.escapeShellArg retiredKeyboardRemapPlist} ]; then
      launchctl asuser "$uid" sudo --user=${lib.escapeShellArg user} -- launchctl bootout "gui/$uid/${retiredKeyboardRemapLabel}" >/dev/null 2>&1 || true
      sudo --user=${lib.escapeShellArg user} -- rm -f ${lib.escapeShellArg retiredKeyboardRemapPlist}
      hidutil property --set '{"UserKeyMapping":[]}' > /dev/null
    fi
  '';

  # sudo の PAM 設定を nix-darwin で管理し、Touch ID 認証を通常端末と tmux/screen の両方で使えるようにする。
  #
  # guard が何を判定して何を止めるかは `nix/pam-touchid-session-guard/pam_touchid_session_guard.c` に書く。
  # ここが決めるのは配置だけである。判定は pam_tid.so より先に走らなければ意味がないため、nix-darwin が
  # `touchIdAuth` / `reattach` から生成する 2 行の前へ `lib.mkBefore` で置く。この 2 つの option はそのまま
  # 有効で、Touch ID は別利用者が console に居ないときの通常経路として残る。
  security.pam.services.sudo_local = {
    touchIdAuth = true;
    reattach = true;
    text = lib.mkBefore "auth       optional       ${pamTouchIdSessionGuard}/lib/pam/pam_touchid_session_guard.so";
  };

  users.users.${user} = {
    home = "/Users/${user}";
    shell = pkgs.zsh;
  };

  system.primaryUser = user;

  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.backupFileExtension = "before-home-manager";
  home-manager.extraSpecialArgs = {
    inherit
      inputs
      root
      user
      includeSelfPackage
      ;
  };
  # `dotfiles switch home` は standalone の `homeConfigurations`（`nix/home.nix`）だけを適用し、Home Manager の
  # generation は darwin 経由の適用と共有される。darwin 層でしか宣言されていない home 生成物は、home 層だけを
  # 適用した時点で旧世代のリンクとして撤去される。利用者の生成物は `nix/home.nix` の import 側で宣言する。
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

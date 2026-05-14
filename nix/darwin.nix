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
  ...
}:
{
  imports = [
    ./modules/macos.nix
    ./modules/homebrew.nix
    ./modules/macos-defaults.nix
    ./modules/launchagents.nix
  ];

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

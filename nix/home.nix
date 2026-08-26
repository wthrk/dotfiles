# Home Manager で管理するユーザー環境の最上位モジュール。
#
# 引数 `user` は `home.username` とホームパスに使う。`root` と `inputs` は
# `shell-files.nix` や `cli.nix` など、必要な子モジュールへ flake ラッパー経由で渡される。
# このモジュールを評価すると、シェル、Git、Neovim、言語ツール、設定ファイルリンクが
# そのユーザーの Home Manager 設定として有効になる。
{ pkgs, user, ... }:
{
  imports = [
    ./modules/cli.nix
    ./modules/launch-agents.nix
    ./modules/colima.nix
    ./modules/languages.nix
    ./modules/zsh.nix
    ./modules/gpg.nix
    ./modules/git.nix
    ./modules/neovim.nix
    ./modules/editor-apps.nix
    ./modules/hammerspoon.nix
    ./modules/shell-files.nix
    ./modules/app-configs.nix
    ./modules/direnv.nix
  ];

  home.username = user;
  # ホームの置き場は OS で違う。生成物にはこの値が焼き込まれるので、実行環境の実ホームと
  # 食い違うと起動した zsh が存在しないパスへ書きに行く。
  home.homeDirectory = if pkgs.stdenv.isDarwin then "/Users/${user}" else "/home/${user}";
  home.stateVersion = "24.11";

  programs.home-manager.enable = true;
}

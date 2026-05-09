# Home Manager で管理するユーザー環境の最上位モジュール。
#
# 引数 `user` は `home.username` と `/Users/<user>` のホームパスに使う。`root` と `inputs` は
# `shell-files.nix` や `cli.nix` など、必要な子モジュールへ flake ラッパー経由で渡される。
# このモジュールを評価すると、シェル、Git、Neovim、言語ツール、設定ファイルリンクが
# そのユーザーの Home Manager 設定として有効になる。
{ user, ... }:
{
  imports = [
    ./modules/cli.nix
    ./modules/languages.nix
    ./modules/zsh.nix
    ./modules/git.nix
    ./modules/neovim.nix
    ./modules/editor-apps.nix
    ./modules/shell-files.nix
    ./modules/app-configs.nix
    ./modules/direnv.nix
  ];

  home.username = user;
  home.homeDirectory = "/Users/${user}";
  home.stateVersion = "24.11";

  programs.home-manager.enable = true;
}

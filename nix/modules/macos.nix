# macOS ホスト全体の宣言のうち、マシンの全ユーザーへ効かせるもの。
#
# font 配置は home 層にもある（Home Manager の `modules/targets/darwin/fonts.nix` が `home.packages` の
# font を `~/Library/Fonts/HomeManager` へ置く）。`fonts.packages` を system 層に残すのは、置き場が
# `/Library/Fonts/Nix Fonts` でマシンの全ユーザーへ効くからであって、機構が nix-darwin にしか
# 無いからではない。
# `environment.systemPackages` は宣言しない。理由は `gui-apps.nix` にある。
# GUI アプリの正本は home 層の `gui-apps.nix`、CLI の正本は `cli.nix` である。
{ pkgs, ... }:
{
  fonts.packages = with pkgs; [
    noto-fonts-color-emoji
    nerd-fonts.zed-mono
  ];
}

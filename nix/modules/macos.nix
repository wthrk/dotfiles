# macOS ホスト全体の宣言のうち、マシンの全ユーザーへ効かせるもの。
#
# font 配置は home 層にもある（Home Manager の `modules/targets/darwin/fonts.nix` が `home.packages` の
# font を `~/Library/Fonts/HomeManager` へ置く）。`fonts.packages` を system 層に残すのは、置き場が
# `/Library/Fonts/Nix Fonts` でマシンの全ユーザーへ効くからであって、機構が nix-darwin にしか
# 無いからではない。
# `environment.systemPackages` は宣言しない。system 層のアプリは nix-darwin が `/Applications/Nix Apps`
# へ bundle ごと実体コピーし、上流が無条件に足す App Management 検査が daemon で
# `permission denied when trying to update apps over SSH` として activation を中断する
# （`/var/log/org.dotfiles.auto-update.err.log` に記録あり）。
# GUI アプリは `homebrew.nix` の casks、CLI の正本は `cli.nix` である。
{ pkgs, ... }:
{
  fonts.packages = with pkgs; [
    noto-fonts-color-emoji
    nerd-fonts.zed-mono
  ];
}

# `home.packages` に入れる GUI アプリ群。
#
# 置き場は `targets.darwin.linkApps` が作る `~/Applications/Home Manager Apps` で、実体は
# `home-manager-applications` の buildEnv への symlink 1 本、その中身も store の bundle への symlink である。
# 適用時に bundle 内部へ書き込む処理が無いため、非 Aqua セッション（auto-update daemon）でも
# App Management 権限を要求しない。
#
# `environment.systemPackages` へは入れない。system 層のアプリは nix-darwin が `/Applications/Nix Apps`
# へ bundle ごと実体コピーし、上流が無条件に足す App Management 検査が daemon で
# `permission denied when trying to update apps over SSH` として activation を中断する
# （`/var/log/org.dotfiles.auto-update.err.log` に記録あり）。
{ lib, pkgs, ... }:
{
  # 置き場の機構を明示する。`home.stateVersion = "24.11"` では既定でこちらが有効だが、既定は
  # stateVersion で切り替わり、実体コピー方式の `copyApps` は bundle への書き込みを伴う。
  targets.darwin.linkApps.enable = pkgs.stdenv.isDarwin;

  # Darwin でだけ宣言する。`iterm2` / `notion-app` / `xquartz` は Linux に無く、unfree な GUI を
  # Linux の activation closure へ持ち込むと CI の `switch home`（x86_64-linux）が実ビルドを踏む。
  home.packages = lib.optionals pkgs.stdenv.isDarwin (
    with pkgs;
    [
      discord
      firefox-bin
      google-chrome
      iterm2
      notion-app
      slack
      vscode
      xquartz
      zed-editor
    ]
  );
}

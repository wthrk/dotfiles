# `home.packages` に入れる CLI ツール群。
#
# 引数 `pkgs` の属性有無を見て、Darwin/Linux や nixpkgs 更新で存在しないパッケージを落とす。
# `inputs` が渡された場合は、この flake がビルドした `dotfiles` CLI を同じユーザー環境へ入れる。
{
  inputs ? null,
  lib,
  pkgs,
  ...
}:
let
  has = lib.hasAttrByPath;
  get = lib.getAttrFromPath;

  optionalPkg = path: if has path pkgs then [ (get path pkgs) ] else [ ];
  system = pkgs.stdenv.hostPlatform.system;
  dotfilesPackage =
    if
      inputs != null
      && inputs ? self
      && inputs.self ? packages
      && has [ system "default" ] inputs.self.packages
    then
      inputs.self.packages.${system}.default
    else
      null;

  gcloudPackage =
    if has [ "google-cloud-sdk" "components" "gke-gcloud-auth-plugin" ] pkgs then
      pkgs.google-cloud-sdk.withExtraComponents (
        with pkgs.google-cloud-sdk.components; [ gke-gcloud-auth-plugin ]
      )
    else if has [ "google-cloud-sdk" ] pkgs then
      pkgs.google-cloud-sdk
    else
      null;
in
{
  programs.gh.enable = true;
  programs.atuin.enable = true;
  programs.zoxide.enable = true;

  programs.fzf = {
    enable = true;
    enableZshIntegration = false;
  };

  home.packages =
    with pkgs;
    [
      android-tools
      awscli2
      automake
      cmake
      colima
      coreutils
      docker
      docker-buildx
      docker-compose
      docker-credential-helpers
      eza
      fd
      ffmpeg
      glow
      graphviz
      helix
      jq
      kubectl
      kubectx
      jujutsu
      mariadb.client
      postgresql_14
      pkgconf
      ripgrep
      skaffold
      sqlite-interactive
      stylua
      tesseract
      tree
      uv
      vips
      wget
      yt-dlp
      zsh-completions
      chromedriver
      silver-searcher
      gnumake
    ]
    ++ lib.optional (dotfilesPackage != null) dotfilesPackage
    ++ lib.optional (gcloudPackage != null) gcloudPackage
    ++ optionalPkg [ "tart" ]
    ++ optionalPkg [ "temurin-bin" ]
    ++ optionalPkg [
      "php84Packages"
      "composer"
    ]
    ++ lib.optionals (!(has [ "php84Packages" "composer" ] pkgs) && has [ "composer" ] pkgs) [
      pkgs.composer
    ];
}

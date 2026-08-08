# `home.packages` に入れる CLI ツール群。
#
# 引数 `pkgs` の属性有無を見て、Darwin/Linux や nixpkgs 更新で存在しないパッケージを落とす。
# `includeSelfPackage = true` かつ `inputs.self.packages` が存在する場合だけ、この flake がビルドした
# `dotfiles` CLI を同じユーザー環境へ入れる。
{
  includeSelfPackage ? true,
  inputs ? null,
  lib,
  pkgs,
  ...
}:
let
  has = lib.hasAttrByPath;
  get = lib.getAttrFromPath;

  # 属性が在っても `meta.platforms` の外なら nixpkgs が評価を拒否する。`tart` のように macOS だけの
  # パッケージがこれに当たるため、属性の有無ではなく評価対象 system で使えるかで採否を決める。
  optionalPkg =
    path:
    let
      package = get path pkgs;
    in
    lib.optionals (has path pkgs && lib.meta.availableOn pkgs.stdenv.hostPlatform package) [ package ];
  system = pkgs.stdenv.hostPlatform.system;
  dotfilesPackage =
    if
      includeSelfPackage
      && inputs != null
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
  # zsh 統合は `config/zsh/completion.zsh` の guard 付き init に一本化する。Home Manager 側の
  # 統合も有効にすると init が二重に走り、後勝ちで `--disable-up-arrow` のような指定が消える。
  programs.atuin = {
    enable = true;
    enableZshIntegration = false;
  };
  programs.zoxide = {
    enable = true;
    enableZshIntegration = false;
  };

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
      age
      bitwarden-cli
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
      gnupg
      graphviz
      helix
      jq
      kubectl
      kubectx
      jujutsu
      mariadb.client
      postgresql_14
      pass
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
      yubikey-manager
      yt-dlp
      zsh-completions
      chromedriver
      gnumake
    ]
    ++ lib.optional (dotfilesPackage != null) dotfilesPackage
    ++ lib.optional (gcloudPackage != null) gcloudPackage
    ++ lib.optionals pkgs.stdenv.isDarwin [
      pinentry_mac
    ]
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

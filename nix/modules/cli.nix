# `home.packages` に入れる CLI ツール群。
#
# 引数 `pkgs` の属性有無を見て、Darwin/Linux や nixpkgs 更新で存在しないパッケージを落とす。
# `includeSelfPackage = true` かつ `inputs.self.packages` が存在する場合だけ、この flake がビルドした
# `dotfiles` CLI を同じユーザー環境へ入れる。
#
# 引数 `bunxTool` は `nix/modules/languages.nix` が `_module.args` で渡す bunx wrapper 生成器で、
# nixpkgs に無い npm 配布の CLI をここで宣言するために使う。
{
  bunxTool,
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

  # `mmdc`（`@mermaid-js/mermaid-cli`）へ渡す Chrome の実行ファイル。
  #
  # `pkgs.google-chrome` は unfree で、`meta.platforms` も aarch64-darwin / aarch64-linux / x86_64-linux に
  # 限られる。無条件に参照すると次の 3 つが同時に壊れる。
  #   1. x86_64-linux の activation closure に unfree Chrome が入り、`switch home` を走らせる CI 2 本
  #      （`static-checks.yml` / `nightly-update.yml`）が Chrome の実ビルドを踏む。
  #   2. `meta.platforms` に含まれない x86_64-darwin で評価が落ちる。
  #   3. `allowUnfree = false` の `pkgs` を渡す利用側 flake で、`home.packages` リスト全体が
  #      「Refusing to evaluate package 'google-chrome-…'」で評価不能になる。
  # そのため Darwin、unfree 許可、対象 system で利用可能の 3 条件が揃った層でだけ参照する。判定順は
  # `&&` の短絡に依存しており、unfree 不許可の層では `meta` にも触れない。
  #
  # 焼き込まれるのは `nix/modules/macos.nix` が `environment.systemPackages` に入れている実体ではなく、
  # ここで解決した `pkgs.google-chrome` 自身の store path である（`macos.nix` は nix-darwin 側のモジュールで、
  # この Home Manager モジュールの評価には関与しない）。実機の aarch64-darwin は 3 条件を満たすため、
  # 従来どおり `mmdc` へ Chrome が渡る。
  puppeteerChrome =
    if
      pkgs.stdenv.isDarwin
      && (pkgs.config.allowUnfree or false)
      && has [ "google-chrome" ] pkgs
      && lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.google-chrome
    then
      pkgs.google-chrome
    else
      null;

  # nixpkgs に無い、または nixpkgs 版を採らない npm 配布の CLI。bunx wrapper として PATH に置く。
  # `mmdc` は Chrome を解決できた層でだけ入れる。渡せないまま wrapper を置いても puppeteer が
  # 起動時に Chrome を見つけられず必ず失敗するため、宣言ごと落とす。
  npmTools = [
    (bunxTool {
      bin = "codex";
      package = "@openai/codex";
    })
    (bunxTool { bin = "difit"; })
  ]
  ++ lib.optional (puppeteerChrome != null) (bunxTool {
    bin = "mmdc";
    package = "@mermaid-js/mermaid-cli";
    # `@mermaid-js/mermaid-cli` は描画に puppeteer の Chromium を使うが、その取得は puppeteer の
    # postinstall で走る。bun は lifecycle script を既定で実行しないため取得自体が起きない。
    # 解決済み Chrome を実行ファイルとして渡し、取得を止める。
    env = {
      PUPPETEER_SKIP_DOWNLOAD = "1";
      PUPPETEER_EXECUTABLE_PATH = lib.getExe' puppeteerChrome "google-chrome-stable";
    };
  });

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
    ++ npmTools
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

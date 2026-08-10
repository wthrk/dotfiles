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

  # npm 配布の CLI を `bunx` 呼び出しの薄い wrapper として PATH 上に置く。
  #
  # alias ではなく PATH 上の実体にする。`nix develop` / direnv でプロジェクトの devShell に入ったとき、
  # プロジェクト側が固定した同名 binary が PATH 前方に来てそのまま優先されてほしいためである。alias は
  # PATH 解決より先に効くので、devShell に入っても利用者環境側を掴み続けてプロジェクトの固定版を潰す。
  #
  # bin 名と npm package 名が一致しない CLI があるため、`--package` で package 名を明示する。
  #
  # exec の前に wrapper 自身のディレクトリを PATH から外す。版指定のない `bunx` は `--package` を
  # 付けていても PATH 上の同名 binary を先に exec するため（bun 1.3.13 で確認）、`home.packages` 経由で
  # `~/.nix-profile/bin/<bin>` や `/etc/profiles/per-user/<user>/bin/<bin>` に置かれた wrapper 自身を
  # bunx が掴み、wrapper が自分を再実行し続ける。除去は PATH 要素、すなわちディレクトリ単位で行い、`<bin>` を
  # 辿るとこの wrapper と同じ store path へ行き着く要素を丸ごと落とす。この構成では `~/.nix-profile/bin` と
  # `/etc/profiles/per-user/<user>/bin` がそれに当たり、同じディレクトリに同居する git や ripgrep も、
  # 起動される CLI からは見えない。同名 binary を持たない残りの PATH 要素は保つ。
  bunxTool =
    {
      bin,
      package ? bin,
      env ? { },
    }:
    let
      # 判定に使うコマンドは PATH に依存しない store path で参照する。
      realpath = lib.getExe' pkgs.coreutils "realpath";
    in
    pkgs.writeShellScriptBin bin ''
      ${lib.concatStringsSep "\n" (
        lib.mapAttrsToList (name: value: "export ${name}=${lib.escapeShellArg value}") env
      )}
      self=$(${realpath} -- ${lib.escapeShellArg "${builtins.placeholder "out"}/bin/${bin}"})
      IFS=':' read -r -a pathElements <<< "$PATH"
      filteredPath=
      for pathElement in "''${pathElements[@]}"; do
        sibling="$pathElement/"${lib.escapeShellArg bin}
        if [ -e "$sibling" ] && [ "$(${realpath} -- "$sibling")" = "$self" ]; then
          continue
        fi
        filteredPath="$filteredPath:$pathElement"
      done
      export PATH="''${filteredPath#:}"
      exec ${lib.getExe' pkgs.bun "bunx"} --package ${lib.escapeShellArg package} ${lib.escapeShellArg bin} "$@"
    '';

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

  # bunx wrapper として PATH に置く npm 配布の CLI。
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

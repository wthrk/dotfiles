# ユーザー環境へ入れる言語ツールチェーン。
#
# nixpkgs の属性名が更新で変わるものは候補を順に選ぶ。rbenv、pyenv、nodebrew などの
# ホーム配下 shim に頼らず、PATH は Nix 由来のツールを優先する前提にする。
#
# nixpkgs 版を入れていた npm 配布の言語ツール（`tsc` / `prettier` / `eslint` / `markdownlint`）は
# `bunxTool` の wrapper へ置き換える（理由は `bunxTool` のコメント）。`bunxTool` 自体は bun を宣言する
# このモジュールが持ち、`_module.args` で兄弟モジュールへ渡す。
{
  inputs,
  lib,
  pkgs,
  ...
}:
let
  has = lib.hasAttrByPath;
  get = lib.getAttrFromPath;
  optionalPkg = path: if has path pkgs then [ (get path pkgs) ] else [ ];

  # npm 配布の CLI を `bunx` 呼び出しの薄い wrapper として PATH 上に置く。
  #
  # alias ではなく PATH 上の実体にする。`nix develop` / direnv でプロジェクトの devShell に入ったとき、
  # プロジェクト側が固定した同名 binary が PATH 前方に来てそのまま優先されてほしいためである。alias は
  # PATH 解決より先に効くので、devShell に入っても利用者環境側を掴み続けてプロジェクトの固定版を潰す。
  # bunx 自身も `node_modules/.bin` を先に見るため、プロジェクトが版を固定していればそれが使われる。
  #
  # `--package` は常に明示する。bin 名と npm package 名が一致しない CLI があり、省略すると bunx が
  # bin 名と同名の別 package を registry から引く。
  bunxTool =
    {
      bin,
      package ? bin,
      env ? { },
    }:
    pkgs.writeShellScriptBin bin ''
      ${lib.concatStringsSep "\n" (
        lib.mapAttrsToList (name: value: "export ${name}=${lib.escapeShellArg value}") env
      )}
      exec ${lib.getExe' pkgs.bun "bunx"} --package ${lib.escapeShellArg package} ${lib.escapeShellArg bin} "$@"
    '';

  # bunx へ寄せる npm 由来の言語ツール。flake.lock による版固定と nightly の版差分追跡からは外れる。
  # ここに置くのは、同じ位置にあった nixpkgs 版（typescript / prettier / eslint / markdownlint-cli）の
  # 置換だけである。言語ツールチェーンでない npm 由来 CLI は `nix/modules/cli.nix` が持つ。
  #
  # `node` / `npm` / `npx` はここに含めない。`npx` を wrapper で潰すと npx 固有の挙動が要る場面の逃げ道が
  # 無くなる。`typescript-language-server` も含めない。mason が自前で取得・管理する層で、二重管理になる。
  npmTools = [
    (bunxTool { bin = "prettier"; })
    (bunxTool { bin = "eslint"; })
    (bunxTool {
      bin = "tsc";
      package = "typescript";
    })
    (bunxTool {
      bin = "markdownlint";
      package = "markdownlint-cli";
    })
  ];

  node = if has [ "nodejs_22" ] pkgs then pkgs.nodejs_22 else pkgs.nodejs;
  pythonBase =
    if has [ "python313" ] pkgs then
      pkgs.python313
    else if has [ "python312" ] pkgs then
      pkgs.python312
    else
      pkgs.python3;
  python = pythonBase.withPackages (
    ps: with ps; [
      pip
      virtualenv
      ipython
      pytest
      pynvim
      requests
    ]
  );
  ruby = if has [ "ruby_3_3" ] pkgs then pkgs.ruby_3_3 else pkgs.ruby;
  go = if has [ "go_1_25" ] pkgs then pkgs.go_1_25 else pkgs.go;
  # rust-overlay の最新 stable ツールチェーン（rustc / cargo / clippy / rustfmt / rust-std）。
  # devShell と flake の buildRustPackage も同じ `stable.latest.default` を使うため、開発環境と
  # 利用者環境で版がずれない。nixpkgs 側の rustc / cargo / clippy / rustfmt は PATH 衝突を避けるため併置しない。
  #
  # `pkgs.rust-bin`（overlay 経由）ではなく `lib.mkRustBin` で `pkgs` から直接組み立てる。このモジュールは
  # `lib.homeManagerModules.default` として公開しており、利用側 flake が自前の `pkgs` を渡す使い方を壊さない
  # ため、overlay 適用済み `pkgs` を前提にしない。`mkRustBin` は既存 `pkgs` へ非侵襲に `rust-bin` 相当を
  # 構築する rust-overlay の公開 API であり、`packages.*`（上流が未安定と明記）には依存しない。
  rustToolchain = (inputs.rust-overlay.lib.mkRustBin { } pkgs).stable.latest.default;
in
{
  # `bunxTool` は bun を宣言するこのモジュールが持ち、npm 由来 CLI を宣言する `cli.nix` へ引数として渡す。
  # wrapper の呼び出し規約（`--package` 明示、env の export 位置）を 1 箇所に保ち、モジュールごとに
  # 同じ定義が分岐するのを防ぐ。
  _module.args.bunxTool = bunxTool;

  home.packages = [
    node
    python
    pkgs.bun
    pkgs.pyright
    pkgs.ruff
    pkgs.black
    rustToolchain
    pkgs.rust-analyzer
    pkgs.cargo-audit
    pkgs.cargo-deny
    pkgs.cargo-edit
    pkgs.cargo-llvm-cov
    pkgs.cargo-make
    go
    pkgs.golangci-lint
    pkgs.delve
    pkgs.php
    ruby
    pkgs.ocaml
    pkgs.dune_3
    pkgs.opam
    pkgs.ocamlPackages.utop
  ]
  ++ npmTools
  ++ optionalPkg [ "diesel-cli" ]
  ++ optionalPkg [
    "rubyPackages_3_3"
    "bundler"
  ]
  ++ lib.optionals (
    !(has [ "rubyPackages_3_3" "bundler" ] pkgs) && has [ "rubyPackages" "bundler" ] pkgs
  ) [ pkgs.rubyPackages.bundler ];
}

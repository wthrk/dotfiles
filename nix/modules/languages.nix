# ユーザー環境へ入れる言語ツールチェーン。
#
# nixpkgs の属性名が更新で変わるものは候補を順に選ぶ。rbenv、pyenv、nodebrew などの
# ホーム配下 shim に頼らず、PATH は Nix 由来のツールを優先する前提にする。
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
  ++ optionalPkg [ "typescript" ]
  ++ optionalPkg [ "prettier" ]
  ++ optionalPkg [ "eslint" ]
  ++ optionalPkg [ "markdownlint-cli" ]
  ++ optionalPkg [ "diesel-cli" ]
  ++ optionalPkg [
    "rubyPackages_3_3"
    "bundler"
  ]
  ++ lib.optionals (
    !(has [ "rubyPackages_3_3" "bundler" ] pkgs) && has [ "rubyPackages" "bundler" ] pkgs
  ) [ pkgs.rubyPackages.bundler ];
}

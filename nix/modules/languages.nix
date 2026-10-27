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
  get = lib.getAttrFromPath;
  # nixpkgs は撤去したパッケージの属性名を、撤去理由を `throw` する alias として残す。属性の有無だけで
  # 候補を選ぶと撤去済みの候補を採用して評価が落ちるため、候補は評価して例外を出さないものに限る。
  available =
    path:
    let
      candidate = builtins.tryEval (lib.hasAttrByPath path pkgs && builtins.seq (get path pkgs) true);
    in
    candidate.success && candidate.value;
  optionalPkg = path: if available path then [ (get path pkgs) ] else [ ];

  node = if available [ "nodejs_22" ] then pkgs.nodejs_22 else pkgs.nodejs;
  pythonBase =
    if available [ "python313" ] then
      pkgs.python313
    else if available [ "python312" ] then
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
  ruby = if available [ "ruby_3_3" ] then pkgs.ruby_3_3 else pkgs.ruby;
  go = if available [ "go_1_25" ] then pkgs.go_1_25 else pkgs.go;
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
    !(available [
      "rubyPackages_3_3"
      "bundler"
    ])
    && available [
      "rubyPackages"
      "bundler"
    ]
  ) [ pkgs.rubyPackages.bundler ];
}

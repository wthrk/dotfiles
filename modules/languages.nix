{ lib, pkgs, ... }:
let
  has = lib.hasAttrByPath;
  get = lib.getAttrFromPath;
  optionalPkg = path: if has path pkgs then [ (get path pkgs) ] else [ ];

  node = if has [ "nodejs_22" ] pkgs then pkgs.nodejs_22 else pkgs.nodejs;
  python =
    (if has [ "python311" ] pkgs then pkgs.python311 else pkgs.python3).withPackages (
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
in
{
  home.packages =
    [
      node
      python
      pkgs.bun
      pkgs.pyright
      pkgs.ruff
      pkgs.black
      pkgs.rustc
      pkgs.cargo
      pkgs.cargo-audit
      pkgs.cargo-deny
      pkgs.cargo-edit
      pkgs.cargo-llvm-cov
      pkgs.cargo-make
      pkgs.go_1_23
      pkgs.golangci-lint
      pkgs.delve
      pkgs.php
      ruby
      pkgs.ocaml
      pkgs.dune_3
      pkgs.opam
      pkgs.ocamlPackages.utop
      pkgs.nodePackages.typescript
      pkgs.nodePackages.prettier
      pkgs.nodePackages.eslint
      pkgs.nodePackages.markdownlint-cli
    ]
    ++ optionalPkg [ "corepack" ]
    ++ optionalPkg [ "clippy" ]
    ++ optionalPkg [ "rustfmt" ]
    ++ optionalPkg [ "diesel-cli" ]
    ++ optionalPkg [ "rubyPackages_3_3" "bundler" ]
    ++ lib.optionals (!(has [ "rubyPackages_3_3" "bundler" ] pkgs) && has [ "rubyPackages" "bundler" ] pkgs) [ pkgs.rubyPackages.bundler ];
}

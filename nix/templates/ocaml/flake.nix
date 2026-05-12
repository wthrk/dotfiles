# `nix flake init -t <dotfiles>#ocaml` で使う OCaml 開発用テンプレート。
# OCaml、Dune、opam、utop を現在のシステム向け devShell に入れる。
{
  description = "OCaml devShell";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs =
    { self, nixpkgs }:
    let
      system = builtins.currentSystem;
      pkgs = import nixpkgs { inherit system; };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          ocaml
          dune_3
          opam
          ocamlPackages.utop
        ];
      };
    };
}

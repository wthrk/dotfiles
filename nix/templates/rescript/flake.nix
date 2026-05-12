# `nix flake init -t <dotfiles>#rescript` で使う ReScript 開発用テンプレート。
# Node.js、Bun、TypeScript、ReScript を現在のシステム向け devShell に入れる。
{
  description = "ReScript devShell";
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
          nodejs_22
          bun
          nodePackages.typescript
          rescript
        ];
      };
    };
}

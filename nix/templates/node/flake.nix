# `nix flake init -t <dotfiles>#node` で使う Node.js 開発用テンプレート。
# Node.js 22、Corepack、Bun、TypeScript 周辺ツールを現在のシステム向け devShell に入れる。
{
  description = "Node devShell";
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
          corepack
          bun
          nodePackages.typescript
          nodePackages.prettier
          nodePackages.eslint
        ];
      };
    };
}

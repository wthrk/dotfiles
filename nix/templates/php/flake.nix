# `nix flake init -t <dotfiles>#php` で使う PHP 開発用テンプレート。
# PHP と Composer を現在のシステム向け devShell に入れる。
{
  description = "PHP devShell";
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
          php
          composer
        ];
      };
    };
}

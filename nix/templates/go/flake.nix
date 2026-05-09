# `nix flake init -t <dotfiles>#go` で使う Go 開発用テンプレート。
# Go 本体、lint、debugger を現在のシステム向け devShell に入れる。
{
  description = "Go devShell";
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
          go_1_23
          golangci-lint
          delve
        ];
      };
    };
}

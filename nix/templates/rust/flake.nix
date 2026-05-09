# `nix flake init -t <dotfiles>#rust` で使う Rust 開発用テンプレート。
# compiler、formatter、lint、audit、coverage など、通常の Rust 開発で使う CLI を devShell に入れる。
{
  description = "Rust devShell";
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
          rustc
          cargo
          clippy
          rustfmt
          cargo-audit
          cargo-deny
          cargo-edit
          cargo-llvm-cov
          cargo-make
        ];
      };
    };
}

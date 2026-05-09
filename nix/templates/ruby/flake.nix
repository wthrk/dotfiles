# `nix flake init -t <dotfiles>#ruby` で使う Ruby 開発用テンプレート。
# Ruby 3.3 と Bundler を現在のシステム向け devShell に入れる。
{
  description = "Ruby devShell";
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
          ruby_3_3
          rubyPackages_3_3.bundler
        ];
      };
    };
}

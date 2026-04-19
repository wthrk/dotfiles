{
  description = "Python devShell";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = { self, nixpkgs }:
    let
      system = "aarch64-darwin";
      pkgs = import nixpkgs { inherit system; };
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.uv
          pkgs.pyright
          pkgs.ruff
          pkgs.black
          (pkgs.python311.withPackages (ps: with ps; [
            pip
            virtualenv
            ipython
            pytest
          ]))
        ];
      };
    };
}

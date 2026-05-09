{
  description = "Python devShell";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs =
    { self, nixpkgs }:
    let
      system = builtins.currentSystem;
      pkgs = import nixpkgs { inherit system; };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.uv
          pkgs.pyright
          pkgs.ruff
          pkgs.black
          (
            (
              if pkgs ? python313 then
                pkgs.python313
              else if pkgs ? python312 then
                pkgs.python312
              else
                pkgs.python3
            ).withPackages
            (
              ps: with ps; [
                pip
                virtualenv
                ipython
                pytest
              ]
            )
          )
        ];
      };
    };
}

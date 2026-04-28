{
  description = "Cloud and container CLI devShell";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs =
    { self, nixpkgs }:
    let
      system = "aarch64-darwin";
      pkgs = import nixpkgs { inherit system; };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          awscli2
          google-cloud-sdk
          kubectl
          kubectx
          skaffold
          docker
          docker-compose
        ];
      };
    };
}

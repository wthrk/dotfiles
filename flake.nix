{
  description = "wthrk dotfiles managed by Home Manager + nix-darwin";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    darwin.url = "github:LnL7/nix-darwin";
    darwin.inputs.nixpkgs.follows = "nixpkgs";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    nix-homebrew.url = "github:zhaofengli-wip/nix-homebrew";

    macos-image-templates = {
      url = "github:cirruslabs/macos-image-templates/70cb4395fee576f4fc49e3b58b2538a7d3400c05";
      flake = false;
    };

    homebrew-core = {
      url = "github:homebrew/homebrew-core";
      flake = false;
    };
    homebrew-cask = {
      url = "github:homebrew/homebrew-cask";
      flake = false;
    };
    homebrew-bicep = {
      url = "github:Azure/homebrew-bicep";
      flake = false;
    };
    homebrew-hashicorp = {
      url = "github:hashicorp/homebrew-tap";
      flake = false;
    };
  };

  outputs =
    inputs@{
      nixpkgs,
      darwin,
      home-manager,
      ...
    }:
    let
      homeHosts = [
        {
          name = "default";
          user = "ya";
          system = "aarch64-darwin";
        }
        {
          name = "runner";
          user = "runner";
          system = "aarch64-darwin";
        }
        {
          name = "dotfilesci";
          user = "dotfilesci";
          system = "aarch64-darwin";
        }
      ];

      darwinHosts = [
        {
          name = "default";
          user = "ya";
          system = "aarch64-darwin";
        }
      ];

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

      mkHome =
        {
          user,
          system,
        }:
        home-manager.lib.homeManagerConfiguration {
          pkgs = pkgsFor system;
          modules = [ ./home.nix ];
          extraSpecialArgs = { inherit inputs user; };
        };

      mkDarwin =
        {
          user,
          system,
        }:
        darwin.lib.darwinSystem {
          inherit system;
          modules = [
            ./darwin.nix
            home-manager.darwinModules.home-manager
            inputs.nix-homebrew.darwinModules.nix-homebrew
          ];
          specialArgs = { inherit inputs user; };
        };

      homeEntriesForHost =
        host:
        let
          cfg = {
            inherit (host) user system;
          };
          value = mkHome cfg;
        in
        [
          {
            name = host.name;
            inherit value;
          }
          {
            name = "${host.user}@${host.name}";
            inherit value;
          }
        ];

      darwinEntriesForHost =
        host:
        let
          cfg = {
            inherit (host) user system;
          };
          value = mkDarwin cfg;
        in
        [
          {
            name = host.name;
            inherit value;
          }
        ];

      formatterEntries =
        map
          (system: {
            name = system;
            value = (pkgsFor system).nixfmt;
          })
          [
            "aarch64-darwin"
            "x86_64-linux"
          ];

      packageEntries = map (system: {
        name = system;
        value =
          let
            pkgs = pkgsFor system;
          in
          {
            tart = pkgs.tart;
            packer = pkgs.packer;
            ansible = pkgs.ansible;
          };
      }) [ "aarch64-darwin" ];

      devShellEntries =
        map
          (system: {
            name = system;
            value =
              let
                pkgs = pkgsFor system;
                darwinPackages =
                  if pkgs.stdenv.isDarwin then
                    [
                      pkgs.ansible
                      pkgs.packer
                      pkgs.sshpass
                      pkgs.tart
                    ]
                  else
                    [ ];
              in
              {
                default = pkgs.mkShell {
                  packages =
                    with pkgs;
                    [
                      bash
                      cargo
                      coreutils
                      git
                      gnugrep
                      gnused
                      jq
                      nil
                      nixd
                      ripgrep
                      rustc
                      rustfmt
                      shellcheck
                      zsh
                    ]
                    ++ darwinPackages;
                };
              };
          })
          [
            "aarch64-darwin"
            "x86_64-linux"
          ];
    in
    {
      homeConfigurations = builtins.listToAttrs (builtins.concatMap homeEntriesForHost homeHosts);

      darwinConfigurations = builtins.listToAttrs (builtins.concatMap darwinEntriesForHost darwinHosts);

      packages = builtins.listToAttrs packageEntries;

      devShells = builtins.listToAttrs devShellEntries;

      formatter = builtins.listToAttrs formatterEntries;
    };
}

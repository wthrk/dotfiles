{
  description = "wthrk dotfiles managed by Home Manager + nix-darwin";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    darwin.url = "github:LnL7/nix-darwin";
    darwin.inputs.nixpkgs.follows = "nixpkgs";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    sops-nix.url = "github:Mic92/sops-nix";
    sops-nix.inputs.nixpkgs.follows = "nixpkgs";

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
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
      ];

      homeHosts = [
        {
          name = "ya";
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
        {
          name = "ya-x86_64-darwin";
          user = "ya";
          system = "x86_64-darwin";
        }
      ];

      darwinHosts = [
        {
          name = "ya";
          user = "ya";
          system = "aarch64-darwin";
          aliases = [ "default" ];
        }
        {
          name = "ya-x86_64-darwin";
          user = "ya";
          system = "x86_64-darwin";
          aliases = [ ];
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
            inputs.sops-nix.darwinModules.sops
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
        ]
        ++ map (alias: {
          name = alias;
          inherit value;
        }) host.aliases;

      formatterEntries = map (system: {
        name = system;
        value = (pkgsFor system).nixfmt;
      }) [
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
            tart-macos-install = pkgs.writeShellApplication {
              name = "tart-macos-install";
              runtimeInputs = with pkgs; [
                ansible
                bash
                coreutils
                git
                gnugrep
                gnused
                jq
                packer
                tart
              ];
              text = ''
                export DOTFILES_MACOS_IMAGE_TEMPLATES_DIR=${inputs.macos-image-templates}
                ${builtins.readFile ./scripts/tart-macos-install.sh}
              '';
            };
            run-macos-install-scenario = pkgs.writeShellApplication {
              name = "run-macos-install-scenario";
              runtimeInputs = with pkgs; [
                bash
                coreutils
                git
                gnugrep
                gnused
                jq
              ];
              text = builtins.readFile ./scripts/run-macos-install-scenario.sh;
            };
            tart = pkgs.tart;
            packer = pkgs.packer;
            ansible = pkgs.ansible;
          };
      }) [ "aarch64-darwin" ];
    in
    {
      homeConfigurations = builtins.listToAttrs (builtins.concatMap homeEntriesForHost homeHosts);

      darwinConfigurations = builtins.listToAttrs (builtins.concatMap darwinEntriesForHost darwinHosts);

      packages = builtins.listToAttrs packageEntries;

      formatter = builtins.listToAttrs formatterEntries;
    };
}

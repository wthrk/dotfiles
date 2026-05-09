{
  description = "wthrk dotfiles managed by Home Manager + nix-darwin";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    darwin.url = "github:LnL7/nix-darwin";
    darwin.inputs.nixpkgs.follows = "nixpkgs";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    nix-homebrew.url = "github:zhaofengli-wip/nix-homebrew";

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
      root = ./.;

      packageSystems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      # system 名から、その環境向けの nixpkgs package set を作る。
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

      homeManagerModule =
        { config, lib, ... }:
        let
          cfg = config.dotfiles;
        in
        {
          imports = [ ./nix/home.nix ];

          options.dotfiles.user = lib.mkOption {
            type = lib.types.str;
            description = "User name for the dotfiles Home Manager configuration.";
          };

          config = {
            _module.args = {
              inherit inputs root;
              user = cfg.user;
            };
          };
        };

      darwinModule =
        { config, lib, ... }:
        let
          cfg = config.dotfiles;
        in
        {
          imports = [
            ./nix/darwin.nix
            home-manager.darwinModules.home-manager
            inputs.nix-homebrew.darwinModules.nix-homebrew
          ];

          options.dotfiles = {
            user = lib.mkOption {
              type = lib.types.str;
              description = "User name for the dotfiles nix-darwin configuration.";
            };
            host = lib.mkOption {
              type = lib.types.str;
              description = "Host name for the dotfiles nix-darwin configuration.";
            };
          };

          config = {
            _module.args = {
              inherit inputs root;
              user = cfg.user;
              host = cfg.host;
            };
          };
        };

      # homeConfigurations の各値を作る関数。user/system を受け取り、
      # ./nix/home.nix を Home Manager module として評価する。
      mkHome =
        {
          user,
          system,
        }:
        home-manager.lib.homeManagerConfiguration {
          pkgs = pkgsFor system;
          modules = [
            homeManagerModule
            { dotfiles.user = user; }
          ];
        };

      # darwinConfigurations の各値を作る関数。user/system を受け取り、
      # ./nix/darwin.nix、Home Manager、nix-homebrew の module をまとめて評価する。
      mkDarwin =
        {
          user,
          host,
          system,
        }:
        darwin.lib.darwinSystem {
          inherit system;
          modules = [
            darwinModule
            {
              dotfiles = {
                inherit user host;
              };
            }
          ];
        };

      mkDotfilesCli =
        pkgs:
        let
          system = pkgs.stdenv.hostPlatform.system;
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "dotfiles-cli";
          version = "0.0.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "--package"
            "dotfiles-cli"
          ];
          cargoTestFlags = [
            "--package"
            "dotfiles-cli"
          ];
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram $out/bin/dotfiles \
              --set-default DOTFILES_HOME_MANAGER ${
                home-manager.packages.${system}.home-manager
              }/bin/home-manager \
              ${pkgs.lib.optionalString pkgs.stdenv.isDarwin "--set-default DOTFILES_DARWIN_REBUILD ${
                darwin.packages.${system}.darwin-rebuild
              }/bin/darwin-rebuild"}
          '';
          meta = {
            mainProgram = "dotfiles";
            description = "dotfiles init and switch CLI";
          };
        };

      # devShells.<system>.default の shell を作る関数。
      mkDevShell = pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.bash
            pkgs.cargo
            pkgs.clippy
            pkgs.coreutils
            pkgs.git
            pkgs.gnugrep
            pkgs.gnused
            pkgs.jq
            pkgs.nil
            pkgs.nixd
            pkgs.ripgrep
            pkgs.rust-analyzer
            pkgs.rustc
            pkgs.rustfmt
            pkgs.shellcheck
            pkgs.zsh
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.ansible
            pkgs.packer
            pkgs.sshpass
            pkgs.tart
          ];
        };
      };

      forSystems =
        f:
        builtins.listToAttrs (
          map (system: {
            name = system;
            value = f system;
          }) packageSystems
        );
    in
    {
      packages = forSystems (
        system:
        let
          pkgs = pkgsFor system;
          dotfiles-cli = mkDotfilesCli pkgs;
        in
        {
          default = dotfiles-cli;
          inherit dotfiles-cli;
        }
        // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
          tart = pkgs.tart;
          packer = pkgs.packer;
          ansible = pkgs.ansible;
        }
      );

      apps = forSystems (system: {
        default = {
          type = "app";
          program = "${mkDotfilesCli (pkgsFor system)}/bin/dotfiles";
          meta.description = "dotfiles init and switch CLI";
        };
      });

      homeManagerModules.default = homeManagerModule;

      darwinModules.default = darwinModule;

      lib = {
        inherit mkHome mkDarwin;
      };

      devShells.aarch64-darwin = mkDevShell (pkgsFor "aarch64-darwin");
      devShells.x86_64-linux = mkDevShell (pkgsFor "x86_64-linux");

      formatter.aarch64-darwin = (pkgsFor "aarch64-darwin").nixfmt;
      formatter.x86_64-linux = (pkgsFor "x86_64-linux").nixfmt;
    };
}

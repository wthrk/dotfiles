# このリポジトリを外部 flake から参照するときの公開面を定義する。
#
# `packages`/`apps` は `dotfiles` CLI を提供する。`homeManagerModules.default` と
# `darwinModules.default` は利用側 flake で `dotfiles.user` / `dotfiles.host` を設定して
# 評価するモジュールであり、`lib.mkHome` / `lib.mkDarwin` は同じ設定を関数引数から生成する。
# 具体的な利用者名、ホスト名、対象システムは、このリポジトリではなく利用側 flake に置く。
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

      # flake 出力を作るときに使う nixpkgs。CLI は macOS と Linux の両方で評価するため、
      # allowUnfree はここで揃えて、各モジュール側で再指定しない。
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

      # 利用側 flake の `homeConfigurations.<name>` に置く評価済み Home Manager 構成を生成する。
      # `user` は Home Manager の `home.username` と出力名、`system` は nixpkgs の評価対象に使う。
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

      # 利用側 flake の `darwinConfigurations.<name>` に置く評価済み nix-darwin 構成を生成する。
      # `user` は primaryUser と Home Manager 対象、`host` は networking.hostName、
      # `system` は nix-darwin の評価対象に使う。
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

      # 開発用シェルは検証コマンドが要求するツールを固定する。Darwin 専用の VM 検証ツールは
      # Linux 評価に混ぜず、対応プラットフォームでだけ入れる。
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
            pkgs.nix
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

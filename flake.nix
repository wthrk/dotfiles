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
      # Home Manager 出力は実ユーザーと CI/runtime シナリオの両方で使う。
      # bootstrap --flake から参照するため、これらの名前は安定させる。
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

      # system 名から、その環境向けの nixpkgs package set を作る。
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

      # homeConfigurations の各値を作る関数。user/system を受け取り、
      # ./home.nix を Home Manager module として評価する。
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

      # darwinConfigurations の各値を作る関数。user/system を受け取り、
      # ./darwin.nix、Home Manager、nix-homebrew の module をまとめて評価する。
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

      # devShells.<system>.default の shell を作る関数。
      mkDevShell =
        pkgs:
        {
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

      # homeHosts の 1 要素から、homeConfigurations に入れる attrset entry を作る。
      homeEntriesForHost =
        host:
        let
          cfg = {
            inherit (host) user system;
          };
          value = mkHome cfg;
        in
        [
          # 手動の home-manager コマンドで .#default と .#ya@default の
          # どちらも使えるように、短い名前と明示名を両方出す。
          {
            name = host.name;
            inherit value;
          }
          {
            name = "${host.user}@${host.name}";
            inherit value;
          }
        ];
    in
    {
      homeConfigurations = builtins.listToAttrs (builtins.concatMap homeEntriesForHost homeHosts);

      # nix-darwin は実 macOS host だけ公開する。CI 専用ユーザーは standalone
      # Home Manager output を使う。
      darwinConfigurations.default = mkDarwin {
        user = "ya";
        system = "aarch64-darwin";
      };

      packages.aarch64-darwin =
        let
          pkgs = pkgsFor "aarch64-darwin";
        in
        {
          tart = pkgs.tart;
          packer = pkgs.packer;
          ansible = pkgs.ansible;
        };

      devShells.aarch64-darwin = mkDevShell (pkgsFor "aarch64-darwin");
      devShells.x86_64-linux = mkDevShell (pkgsFor "x86_64-linux");

      formatter.aarch64-darwin = (pkgsFor "aarch64-darwin").nixfmt;
      formatter.x86_64-linux = (pkgsFor "x86_64-linux").nixfmt;
    };
}

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

    homebrew-core = {
      url = "github:homebrew/homebrew-core";
      flake = false;
    };
    homebrew-cask = {
      url = "github:homebrew/homebrew-cask";
      flake = false;
    };
    homebrew-cask-fonts = {
      url = "github:homebrew/homebrew-cask-fonts";
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

      hosts = [
        {
          name = "ya";
          user = "ya";
          system = "aarch64-darwin";
          aliases = [ "default" ];
        }
        {
          name = "runner";
          user = "runner";
          system = "aarch64-darwin";
          aliases = [ ];
        }
        {
          name = "dotfilesci";
          user = "dotfilesci";
          system = "aarch64-darwin";
          aliases = [ ];
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
        value = (pkgsFor system).nixfmt-rfc-style;
      }) systems;
    in
    {
      homeConfigurations = builtins.listToAttrs (builtins.concatMap homeEntriesForHost hosts);

      darwinConfigurations = builtins.listToAttrs (builtins.concatMap darwinEntriesForHost hosts);

      formatter = builtins.listToAttrs formatterEntries;
    };
}

# このリポジトリを外部 flake から参照するときの公開面を定義する。
#
# `packages`/`apps` は `dotfiles` CLI を提供する。`darwinModules.default` と
# `lib.homeManagerModules.default` は利用側 flake で `dotfiles.user` / `dotfiles.host` を
# 設定して評価するモジュールであり、`lib.mkHome` / `lib.mkDarwin` は同じ設定を関数引数から生成する。
# 具体的な利用者名、ホスト名、対象システムは、このリポジトリではなく利用側 flake に置く。
{
  description = "wthrk dotfiles managed by Home Manager + nix-darwin";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    darwin.url = "github:LnL7/nix-darwin";
    darwin.inputs.nixpkgs.follows = "nixpkgs";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    # Rust ツールチェーンの取得元。nixpkgs の rustc/cargo は channel の都合で stable 最新から遅れるため、
    # upstream の release manifest をそのまま提供する rust-overlay を単一の供給元にする。
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

    nix-homebrew.url = "github:zhaofengli-wip/nix-homebrew";

    homebrew-homebrew-core = {
      url = "github:homebrew/homebrew-core";
      flake = false;
    };
    homebrew-homebrew-cask = {
      url = "github:homebrew/homebrew-cask";
      flake = false;
    };
    homebrew-azure-bicep = {
      url = "github:Azure/homebrew-bicep";
      flake = false;
    };
    homebrew-hashicorp-tap = {
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

      # Homebrew tap の宣言は `homebrew-<owner>-<tap>` の flake input 名に集約し、
      # そこから nix-homebrew と brew bundle の tap 名、clone 先を生成する。
      homebrewTapInputs = nixpkgs.lib.filterAttrs (
        name: _: nixpkgs.lib.hasPrefix "homebrew-" name
      ) inputs;

      homebrewTaps =
        let
          mkHomebrewTap =
            inputName: source:
            let
              inputParts = nixpkgs.lib.splitString "-" (nixpkgs.lib.removePrefix "homebrew-" inputName);
              owner = nixpkgs.lib.elemAt inputParts 0;
              tap = nixpkgs.lib.concatStringsSep "-" (nixpkgs.lib.tail inputParts);
              repo = "homebrew-${tap}";
              name = "${owner}/${tap}";
              nixHomebrewName = "${owner}/${repo}";
            in
            {
              inherit name nixHomebrewName source;
              brewTap = {
                inherit name;
                clone_target = "https://github.com/${nixHomebrewName}";
              };
            };
        in
        nixpkgs.lib.mapAttrsToList mkHomebrewTap homebrewTapInputs;

      packageSystems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      # flake 出力を作るときに使う nixpkgs。CLI は macOS と Linux の両方で評価するため、
      # allowUnfree はここで揃えて、各モジュール側で再指定しない。
      # rust-overlay を適用して `pkgs.rust-bin` を生やし、devShell・buildRustPackage・利用者環境が
      # 同じ Rust ツールチェーンを参照できるようにする。
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          config.allowUnfree = true;
          overlays = [ inputs.rust-overlay.overlays.default ];
        };

      # repo 全体で使う Rust ツールチェーン。`stable.latest` は rust-overlay が持つ release manifest の
      # 最新 stable を指すため、nightly の `nix flake update` で rust-overlay の rev が進むたびに
      # 新しい stable 版へ追従する。`default` profile は rustc / cargo / rustfmt / clippy / rust-std を含み、
      # `cargo xtask check static` が使う `cargo fmt` と `cargo clippy` をこの 1 パッケージで賄う。
      # 利用者環境（`nix/modules/languages.nix`）は同じ `stable.latest.default` を overlay 非依存の
      # `rust-overlay.lib.mkRustBin` から取る。公開している `lib.homeManagerModules.default` に
      # 「overlay 適用済み pkgs」という新しい前提を課さないためで、選ぶツールチェーンは同一である。
      rustToolchainFor = pkgs: pkgs.rust-bin.stable.latest.default;

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

          options.dotfiles.includeSelfPackage = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Whether the dotfiles CLI package is added to home.packages.";
          };

          config = {
            _module.args = {
              inherit inputs root;
              user = cfg.user;
              includeSelfPackage = cfg.includeSelfPackage;
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
            includeSelfPackage = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = "Whether the dotfiles CLI package is added to user packages.";
            };
          };

          config = {
            _module.args = {
              inherit inputs root homebrewTaps;
              user = cfg.user;
              host = cfg.host;
              includeSelfPackage = cfg.includeSelfPackage;
            };
          };
        };

      # 利用側 flake の `homeConfigurations.<name>` に置く評価済み Home Manager 構成を生成する。
      # `user` は Home Manager の `home.username` と出力名、`system` は nixpkgs の評価対象に使う。
      mkHome =
        {
          user,
          system,
          includeSelfPackage ? true,
        }:
        home-manager.lib.homeManagerConfiguration {
          pkgs = pkgsFor system;
          modules = [
            homeManagerModule
            {
              dotfiles = {
                inherit user includeSelfPackage;
              };
            }
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
          includeSelfPackage ? true,
        }:
        darwin.lib.darwinSystem {
          inherit system;
          modules = [
            darwinModule
            {
              dotfiles = {
                inherit user host includeSelfPackage;
              };
            }
          ];
        };

      # cargo のビルド成果物が link する C ライブラリ。この集合を消費するのは実ビルド（`mkDotfilesCli`）、
      # CI が `target/` を生成する devShell（`mkDevShell`）、CI の cache key（`mkCargoBuildInputs`）の 3 箇所で、
      # どれかが別の集合を指すと cache key が `target/` の実際のビルド closure を追わなくなる。定義はここ
      # 1 箇所に置き、3 箇所とも同じ関数を参照する。
      cargoLinkLibraries =
        pkgs:
        [
          # GPG keyring backend（`gpgme` crate）が link する libgpgme / libgpg-error。
          pkgs.gpgme
          # password-store clone backend（`git2` crate）が link する libgit2 / libssh2 と、
          # libssh2 / libgit2 が要求する OpenSSL / zlib。
          pkgs.libgit2
          pkgs.libssh2
          pkgs.openssl
          pkgs.zlib
        ]
        ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.pcsclite ];

      # vendored libgit2 / libssh2 を build script が組み立てるときに起動する native ツール。
      cargoBuildTools = pkgs: [
        pkgs.cmake
        pkgs.pkg-config
      ];

      # `target/` を生成する Rust toolchain。cache key と devShell が別の rustc を指さないよう、
      # `cargoLinkLibraries` と同じ理由でここ 1 箇所に置く。供給元は rust-overlay 由来の
      # `rustToolchainFor` 1 パッケージだけにする。nixpkgs 側の `pkgs.cargo` / `pkgs.rustc` を併置すると
      # PATH 上で衝突し、どちらが使われるか不定になる。
      cargoToolchain = pkgs: [ (rustToolchainFor pkgs) ];

      # CI の rust-cache が復元する `target/` の正しさを縛るのは、build script が link した C ライブラリと
      # ビルドに使った toolchain の store path だけである。この derivation はその集合だけを入力に持ち、CI は
      # `nix path-info --derivation` で得た drv 名を rust-cache の shared-key に使う。`flake.lock` のハッシュを
      # キーにすると nightly bump のたびに必ず別キーになり、devShell 全体をキーにすると rust-analyzer や
      # actionlint のような成果物と無関係なツールの bump でも別キーになる。キーの導出にしか使わないため
      # 成果物は空でよい。
      #
      # CI が `target/` を作るのは `mkDevShell` の devShell であり、その devShell は下で同じ 3 関数から
      # 組む。したがってこの derivation の入力は devShell が持つ link 対象と toolchain の上位集合であり、
      # devShell へライブラリを足せばキーも動く。
      mkCargoBuildInputs =
        pkgs:
        pkgs.runCommand "cargo-build-inputs"
          {
            buildInputs = cargoLinkLibraries pkgs ++ cargoBuildTools pkgs ++ cargoToolchain pkgs;
          }
          ''
            touch "$out"
          '';

      mkDotfilesCli =
        pkgs:
        let
          system = pkgs.stdenv.hostPlatform.system;
          inherit (pkgs.lib) fileset;
          rustToolchain = rustToolchainFor pkgs;
          # devShell と同じツールチェーンでビルドする。nixpkgs 既定の `rustPlatform` を使うと
          # 開発時の rustc と成果物の rustc が食い違い、devShell で通った lint/edition がビルドで落ちる。
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        in
        rustPlatform.buildRustPackage {
          pname = "dotfiles-cli";
          version = "0.0.0";
          # src は Rust ビルドが読む範囲に限定する。`./.` にすると derivation hash が全 tracked file の関数に
          # なり、docs / workflow / nix module だけの変更でも別 derivation になる。CI は derivation 名を
          # ビルド成果物キャッシュのキーにしているため、この限定が外れると Rust を触らない PR でも
          # 必ず cache miss してフルビルドに戻る。
          src = fileset.toSource {
            root = ./.;
            fileset = fileset.unions [
              ./.cargo
              ./Cargo.lock
              ./Cargo.toml
              ./rust
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "--package"
            "dotfiles-cli"
          ];
          # checkPhase は走らせない。テストは devShell 側の `cargo xtask check static` と重複する。
          # この derivation が検出する依存欠落は buildPhase が compile / link する範囲、すなわち下の
          # buildInputs のライブラリと buildPhase が起動する nativeBuildInputs までである。#84 の git の
          # ように checkPhase だけが起動する実行時依存の欠落は、ここでは検出できない。
          doCheck = false;
          buildInputs = cargoLinkLibraries pkgs;
          nativeBuildInputs = cargoBuildTools pkgs ++ [ pkgs.makeWrapper ];
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

      # 開発用シェルは検証コマンドが要求するツールを固定する。cargo が link するライブラリ、build script が
      # 起動する native ツール、Rust toolchain は上の 3 関数から取り、`mkCargoBuildInputs` が導出する cache key
      # と同じ集合にする（ここへ直接書き足すとキーが追わない依存ができる）。Darwin 専用の VM 検証ツールは
      # Linux 評価に混ぜず、対応プラットフォームでだけ入れる。
      mkDevShell = pkgs: {
        default = pkgs.mkShell {
          packages =
            cargoLinkLibraries pkgs
            ++ cargoBuildTools pkgs
            ++ cargoToolchain pkgs
            ++ [
              pkgs.actionlint
              pkgs.bash
              # zsh 設定の実挙動検証（`tests/zsh`）の実行系。assertion library まで含めて固定し、
              # 検証側が runner 上の任意の bats 環境に依存しないようにする。
              (pkgs.bats.withLibraries (libraries: [
                libraries.bats-support
                libraries.bats-assert
              ]))
              pkgs.coreutils
              pkgs.git
              pkgs.gnugrep
              pkgs.gnupg
              pkgs.gnused
              pkgs.jq
              pkgs.nil
              pkgs.nix
              pkgs.nixd
              pkgs.ripgrep
              pkgs.rust-analyzer
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

      # cache key の導出にしか使わない CI 内部成果物なので、利用者向けの `packages` 面には出さない。
      # installable の属性解決は `packages.<system>` の次に `legacyPackages.<system>` を見るため、
      # workflow の `nix path-info --derivation .#cargo-build-inputs` はこの配置でも解決する。
      legacyPackages = forSystems (system: {
        cargo-build-inputs = mkCargoBuildInputs (pkgsFor system);
      });

      apps = forSystems (system: {
        default = {
          type = "app";
          program = "${mkDotfilesCli (pkgsFor system)}/bin/dotfiles";
          meta.description = "dotfiles init and switch CLI";
        };
      });

      darwinModules.default = darwinModule;

      # nightly bump の更新内容を算出するための CI 参照ホスト。
      #
      # 各マシンの concrete な `darwinConfigurations` は利用側 flake に置くため、本 repo 単体には
      # bump 前後で版差分を取る対象が無い。nightly-update.yml はこの固定参照構成の宣言パッケージ
      # （`home.packages` + `environment.systemPackages`）の name/version を bump 前後の lock で
      # `nix eval`（評価時属性 `pname`/`version`、ビルド/フェッチ非実行・数秒）して版差分を算出する。
      # つまり ci-ref は「eval で宣言パッケージ版を取得する参照構成」であり closure は build しない。
      # 利用者名・ホスト名は CI 内の固定値であり、実フリートとは独立した参照専用構成である。
      darwinConfigurations.ci-ref = mkDarwin {
        user = "ci";
        host = "ci-ref";
        system = "aarch64-darwin";
      };

      # zsh 設定の実挙動検証（`tests/zsh`）が起動する Home Manager 構成。
      #
      # 各マシンの concrete な `homeConfigurations` は利用側 flake に置くため、本 repo 単体には
      # 検証が build できる評価済み home 構成が無い。`dotfiles init` が生成する flake を検証のたびに
      # 作って評価する方式だと、検証経路が dotfiles CLI のビルドを要求し、zsh 挙動の検証に Rust の
      # ビルド成果物が必要になる。`dotfiles init` 経路自体は runtime 統合検証（VM）が実行するため、
      # zsh 検証はこの参照構成だけを見る。
      #
      # `includeSelfPackage = false` は、この検証が dotfiles CLI の release build を払わないための
      # 宣言であり、`tests/zsh` がその不変条件を検査する。利用者名は CI 内の固定値であり、実フリート
      # とは独立した参照専用構成である。
      homeConfigurations.ci-ref = mkHome {
        user = "ci";
        system = "aarch64-darwin";
        includeSelfPackage = false;
      };

      lib = {
        inherit mkHome mkDarwin;
        homeManagerModules.default = homeManagerModule;
      };

      devShells.aarch64-darwin = mkDevShell (pkgsFor "aarch64-darwin");
      devShells.x86_64-linux = mkDevShell (pkgsFor "x86_64-linux");

      formatter.aarch64-darwin = (pkgsFor "aarch64-darwin").nixfmt-tree;
      formatter.x86_64-linux = (pkgsFor "x86_64-linux").nixfmt-tree;
    };
}

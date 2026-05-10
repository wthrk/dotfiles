# dotfiles

Nix flake として再利用できる `nix-darwin` + `home-manager` 管理 dotfiles。

## 開発環境

初回だけ許可します。

```sh
direnv allow .
```

このディレクトリでは `direnv` が flake の devShell を読み込みます。検証や内部開発タスクは `cargo xtask` で実行します。

```sh
cargo xtask check
```

## CLI

ユーザー向けの入口は `dotfiles` コマンドです。ユーザー固有の flake は `~/.config/dotfiles/flake.nix` に生成します。

```sh
nix run github:wthrk/dotfiles -- init
nix run github:wthrk/dotfiles -- init --user alice --host macbook --system aarch64-darwin
nix run github:wthrk/dotfiles -- init --source github:wthrk/dotfiles --force
```

適用:

```sh
dotfiles switch home
dotfiles switch darwin
dotfiles switch all
```

まだ `dotfiles` が PATH にない初回は `nix run github:wthrk/dotfiles -- switch darwin` のように実行できます。

実行される switch:

```sh
home-manager switch --flake ~/.config/dotfiles#<user>
sudo darwin-rebuild switch --flake ~/.config/dotfiles#<host>
```

## 内部タスク

`xtask` はこの repository の開発用です。

```sh
cargo xtask apply
```

Home Manager のみ部分適用:

```sh
cargo xtask apply home-manager
```

## 初回導入

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/scripts/bootstrap.sh | bash
```

commit / tag 固定:

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/<tag-or-commit>/scripts/bootstrap.sh | DOTFILES_BOOTSTRAP_SOURCE_REF=<tag-or-commit> bash
```

主な option:

- `--source` dotfiles flake
- `--user`
- `--host`
- `--system`
- `--force`
- `--mode darwin|home-manager|all`
- `--no-switch`

bootstrap は Nix を用意して `dotfiles init` / `dotfiles switch` を呼びます。`~/.dotfiles` checkout は前提にしません。既存の Home Manager 管理対象ファイルは `*.before-home-manager` に退避してから置き換えます。

## 検証

```sh
cargo xtask check
```

`cargo xtask check` は Rust の format/check/clippy/test、flake check、Nix format、Home Manager output 評価、zsh 挙動検証を実行します。Tart VM を使う runtime 検証は含めません。

すべて実行する場合:

```sh
cargo xtask check all
```

個別に実行する場合:

```sh
cargo xtask check static
cargo xtask check zsh
```

Tart VM を使う runtime 検証:

```sh
cargo xtask check runtime
```

## ロールバック

`nix-darwin`:

```sh
sudo darwin-rebuild --list-generations
sudo darwin-rebuild switch --rollback
```

Home Manager:

```sh
home-manager generations
home-manager switch --rollback
```

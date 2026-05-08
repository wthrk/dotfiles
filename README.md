# dotfiles

`ya` 用の `nix-darwin` + `home-manager` 管理 dotfiles。

## 開発環境

初回だけ許可します。

```sh
direnv allow .
```

このディレクトリでは `direnv` が flake の devShell を読み込みます。検証や補助タスクは `cargo xtask` で実行します。

```sh
cargo xtask check
```

## 適用

すべて適用:

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
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/<tag-or-commit>/scripts/bootstrap.sh | bash
```

dry-run:

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/scripts/bootstrap.sh | bash -s -- --dry-run
```

主な option:

- `--dir` checkout 先
- `--flake` flake 出力名
- `--mode darwin|home-manager`
- `--no-switch`
- `--dry-run`

既存の Home Manager 管理対象ファイルは `*.before-home-manager` に退避してから置き換えます。

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
cargo xtask check runtime all
cargo xtask check runtime fresh-bootstrap
cargo xtask check runtime second-user-home-manager
cargo xtask check runtime darwin-switch-ya
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

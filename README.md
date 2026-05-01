# dotfiles

`ya` 用の `nix-darwin` + `home-manager` 管理 dotfiles。

## 適用

通常:

```sh
nix develop -c cargo xtask check static
sudo darwin-rebuild switch --flake .#default
```

Home Manager のみ:

```sh
home-manager switch --flake .#default
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
nix develop -c cargo xtask check static
nix develop -c cargo xtask check zsh
```

```sh
nix develop -c cargo xtask check runtime fresh-bootstrap
nix develop -c cargo xtask check runtime second-user-home-manager
nix develop -c cargo xtask check runtime darwin-switch-ya
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

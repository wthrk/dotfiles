# dotfiles

`ya` 用の `nix-darwin` + `home-manager` 管理 dotfiles。

## 適用

通常:

```sh
nix flake check --no-update-lock-file
sudo darwin-rebuild switch --flake .#ya
```

Home Manager のみ:

```sh
home-manager switch --flake .#ya
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
bash scripts/verify-nix-migration.sh
```

主な option:

- `--phase pre-switch|post-migration`
- `--flake NAME`

追加検証:

```sh
bash scripts/test-zsh-shortcuts.sh
bash scripts/test-zsh-key-operations-full.sh
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

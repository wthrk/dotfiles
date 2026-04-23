# dotfiles

`ya` ユーザー向けの Home Manager + nix-darwin 管理 dotfiles。

## 適用手順

統合モード（既定）:

```sh
nix flake check
sudo darwin-rebuild switch --flake .#ya
```

Home Manager 単体モード（任意。通常運用では統合モードと混在しない）:

```sh
home-manager switch --flake .#ya
```

## 初回導入（公開 raw エンドポイント）

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/scripts/bootstrap.sh | bash
```

tag/commit 固定 URL:

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/<tag-or-commit>/scripts/bootstrap.sh | bash
```

bootstrap は Nix 優先/flake 専用です。`init.sh` は削除済みで、フォールバックはありません。
既存の Home Manager 管理対象ファイルは `*.before-home-manager` にバックアップしてから置き換えます。

主要な options:

- `--dir`（既定: `~/.dotfiles`）
- `--flake`（既定: `ya`、この移行フェーズの標準）
- `--mode`（`darwin` または `home-manager`、既定: `darwin`）
- `--no-switch`（`nix flake check` まで）
- `--sops-age-key-file`（任意の鍵ファイル）
- `--sops-age-key-dest`（既定: `/var/lib/sops-nix/key.txt`）
- `--dry-run`（実行計画のみ表示して終了）

dry-run 例:

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/scripts/bootstrap.sh | bash -s -- --dry-run
```

## 所有権ルール

- `programs.*`: `git`, `gh`, `neovim`, `direnv`, `zsh`, `fzf`, `atuin`, `zoxide`
- `home.packages`: 単機能の実行バイナリと言語ランタイムツール
- nix-darwin: `homebrew.*`, `fonts`, `launchd`, `system.defaults`, `mas`
- nix-homebrew: Homebrew 本体と taps の所有

## zsh 方針

- Antidote の起動時 clone は廃止。
- plugin は Home Manager `programs.zsh.plugins` で固定。
- `programs.fzf.enableZshIntegration = false`。
- `key-bindings.zsh` は Nix 提供のみを source。
- `^I` は `expand-or-complete`、`^X^I` は `fzf-tab-complete`。
- 優先 PATH から除外する対象: `~/.nodebrew/current/bin`, `~/.bun/bin`, `~/.cargo/bin`, `~/.pyenv/bin`, `~/.rbenv/bin`。
- 管理対象外として許容する PATH: `~/.agent-tools/bin`, `~/.rd/bin`。
- Rancher Desktop の shell injection block（例: `### MANAGED BY RANCHER DESKTOP ...`）は ランタイム/アプリ管理状態 として扱い、repo/HM 管理の `.zshrc` には保持しません。必要なら Rancher Desktop 設定側で再生成・管理します。

## Neovim 方針

- LSP の所有権は editor 側（Mason 利用可）。
- Mason の PATH 挿入は無効（`PATH = "skip"`）。
- formatter/linter の Mason installer は無効（`mason-null-ls` の installer list は空）。
- `mason-lspconfig` の `ensure_installed` で `tsserver` は `ts_ls` へ移行。

## シークレット管理（sops-nix）

既定では雛形のみ用意し、secrets は明示設定まで無効:

- `.sops.yaml`
- `secrets/common.yaml`
- `secrets/hosts/ya.yaml`
- `secrets/users/ya.yaml`

暗号化と鍵配置後に有効化:

- host key: `/var/lib/sops-nix/key.txt`（`0400`）
- user key: `~/.config/sops/age/keys.txt`（`0600`）
- Home Manager 側で `dotfiles.enableSops = true`

ランタイム認証状態は管理対象外（例: `~/.docker/config.json` の auth、`~/.kube/config`、`~/.config/gcloud/*db`、`~/.config/gh/hosts.yml`）。

## 検証

基本検証:

```sh
bash scripts/verify-nix-migration.sh
```

`verify` は pre-switch 状態やローカル状態（例: Neovim parser cache）に応じて `WARN`/`SKIP` を返す場合があります。`--phase post-migration` では Compose plugin の非 Nix 供給元を失敗として扱います。

zsh 検証:

```sh
bash scripts/test-zsh-shortcuts.sh
bash scripts/test-zsh-key-operations-full.sh
```

## 例外

- `openvino` はグローバル既定で導入しません。プロジェクトの devShell/例外運用で扱います。

## ロールバック

統合 nix-darwin モード:

```sh
sudo darwin-rebuild --list-generations
sudo darwin-rebuild switch --rollback
```

Home Manager 単体モード:

```sh
home-manager generations
home-manager switch --rollback
```

## 廃止済み項目

`init.sh` は Nix 移行後に削除済みです。初回導入は `scripts/bootstrap.sh` を使用し、適用は `darwin-rebuild` または `home-manager` の経路を使用してください。

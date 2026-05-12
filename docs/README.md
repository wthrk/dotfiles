# docs 補足

## ランタイム/アプリ管理の shell 変更

Rancher Desktop の shell 注入ブロック（例: `### MANAGED BY RANCHER DESKTOP ...`）や、同種のアプリ管理ランタイム shell 変更は、repo 管理または Home Manager 管理の `.zshrc` には含めません。

これらが必要な場合は、このリポジトリ外でアプリ側（例: Rancher Desktop の設定）から有効化・管理してください。

## Docker のランタイム境界

Docker の ランタイム認証/設定状態（例: `~/.docker/config.json` の認証/セッション/context に関わる可変項目）は dotfiles/Home Manager の管理対象外です。

移行後は `~/.docker/cli-plugins/docker-compose` が Nix 所有の実体へ解決されることを期待値とします。

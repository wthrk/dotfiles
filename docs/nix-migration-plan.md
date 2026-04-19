# Nix 移行計画書

## 目的

この dotfiles と現在の macOS 開発環境を、手動インストール中心の状態から Nix 管理へ移行する。

移行後の状態は次の通り。

- Homebrew formula で管理している CLI は、原則として Nix のパッケージへ移す。
- CLI / daemon / 開発ツールは原則 Nix で管理し、GUI アプリは `Nix で問題なく管理可能なら Nix` -> `MAS が正規経路なら MAS` -> `Nix で破綻する場合だけ理由付きで cask / 手動例外` の順で判定する。
- `$HOME` 配下に手動で入っているツールは、Nix パッケージ、Nix で自作したパッケージ、明示した管理対象外項目へ振り分ける。
- Node、Python、Rust、Ruby、Go、PHP、OCaml などの開発用言語環境は、グローバルでも使える状態で Nix 管理する。
- プロジェクト固有のバージョンや依存は、`nix develop` / devShell で上書きできるようにする。
- LSP は各エディタ/エージェントツール側の管理を優先し、Nix は language runtime / compiler / formatter / linter / CLI / build tool を供給する。
- zsh の plugin は起動時 clone ではなく、Nix/Home Manager 側で固定する。
- PATH は Nix 管理のコマンドを優先し、`nodebrew`、`pyenv`、`rbenv`、`pipx`、`~/.cargo/bin`、`~/.bun/bin` への依存をなくす。
- rollback 手順を Home Manager と nix-darwin で確認する。

## 前提

- この文書は日本語で管理する。
- 入口は単一ユーザーの dotfiles とする。ただし多ユーザー環境へ拡張できるよう、system 設定と user 設定の境界を崩さない。
- 初期適用の flake 入口は `.#ya` を許容する。多ユーザーまたは複数 host へ広げる場合は `darwinConfigurations.<host>` と `homeConfigurations."<user>@<host>"` に分離する。
- 言語環境をグローバルから消さない。グローバルに Nix 管理の言語環境を置き、プロジェクトごとの devShell で上書きできる構成にする。
- 既存の手動導入物は、Nix で置換して検証するまで削除しない。

## 初回導入

新しい macOS 環境では、公開済みの bootstrap endpoint を curl の入口にする。

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/<bootstrap-entrypoint> | bash
```

ブランチ名が `main` ではない場合、または unattended install で再現性を強く要求する場合は、default branch ではなく tag または commit SHA 固定の raw URL を使う。

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/<tag-or-commit>/<bootstrap-entrypoint> | bash
```

bootstrap の責務:

- macOS 以外では失敗する。
- Xcode Command Line Tools がない場合は `xcode-select --install` を起動し、完了後の再実行を要求する。
- dotfiles repository を `DOTFILES_DIR`（既定: `~/.dotfiles`）へ clone する。既存 checkout がある場合はそれを使う。
- Nix がない場合は公式 installer を multi-user daemon mode（`--daemon`）で導入する。
- `DOTFILES_SOPS_AGE_KEY_FILE` が指定された場合だけ、age secret key を `DOTFILES_SOPS_AGE_KEY_DEST`（既定: `/var/lib/sops-nix/key.txt`）へ `0400` で配置する。
- `flake.nix` が存在する場合は `nix flake check` を実行し、`DOTFILES_SWITCH_MODE` に応じて `darwin-rebuild switch` または `home-manager switch` を実行する。
- bootstrap は flake 専用とし、`flake.nix` がない checkout では失敗させる（`init.sh` フォールバックは持たない）。

代表的な実行例:

```sh
# host 名を明示して nix-darwin 構成を適用する
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/<bootstrap-entrypoint> | DOTFILES_FLAKE=ya bash

# switch せず、clone / Nix 導入 / flake check までで止める
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/<bootstrap-entrypoint> | DOTFILES_RUN_SWITCH=0 bash

# 完全 dry-run（実行計画だけ表示して終了。インストール/鍵配置/switch は行わない）
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/<bootstrap-entrypoint> | DOTFILES_DRY_RUN=1 bash

# standalone Home Manager として適用する
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/<bootstrap-entrypoint> | DOTFILES_SWITCH_MODE=home-manager DOTFILES_FLAKE=ya bash

# sops-nix 用 age key を同時に配置する
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/<bootstrap-entrypoint> | DOTFILES_SOPS_AGE_KEY_FILE=/Volumes/Secrets/sops-age-key.txt bash
```

新しい環境での作業順:

1. macOS 初期設定後、ネットワーク接続と Apple ID / App Store signin 要否を確認する。
2. Xcode Command Line Tools を入れる。未導入なら bootstrap が installer を起動する。
3. secret を復号する必要がある host では、age key を外部媒体または 1Password 等から取り出し、`DOTFILES_SOPS_AGE_KEY_FILE=/path/to/key.txt` を指定して bootstrap を実行する。
4. `DOTFILES_RUN_SWITCH=0` で `nix flake check` まで通し、差分と secret 配置を確認する。
5. `DOTFILES_FLAKE=ya` を明示して `darwin-rebuild switch --flake .#ya` 相当を実行する（この移行フェーズの標準）。
6. `command -v zsh git nvim nix`、`nix flake check`、`sudo darwin-rebuild build --flake .#ya`、`home-manager generations` を確認する。

## 現状の棚卸し

### dotfiles

現在の dotfiles は主に以下を管理している。

- `.zshrc`
- `.gitconfig`
- `config/zsh`
- `config/nvim`
- zsh 関連ドキュメントとテストスクリプト

移行前の `init.sh` は以下を symlink していた（現在 `init.sh` は削除済み）。

```text
~/.zshrc        -> .dotfiles/.zshrc
~/.config/zsh  -> .dotfiles/config/zsh
~/.config/nvim -> .dotfiles/config/nvim
~/.gitconfig   -> .dotfiles/.gitconfig
```

移行後、この役割は Home Manager の `home.file` / `xdg.configFile` に移す。

### Homebrew formula

現在 Homebrew formula で管理されているもの。

```text
atuin
automake
awscli
cmake
colima
composer
coreutils
docker
docker-buildx
docker-compose
docker-credential-helper
ffmpeg
fzf
gh
git
glow
go
gobject-introspection
graphviz
helix
jj
jpeg
jq
kubectx
libass
libffi
make
marksman
mysql
mysql-client
neovim
nodebrew
opam
openssl@1.1
openvino
pipx
postgresql@14
pyenv
python@3.10
python@3.11
python@3.9
rbenv-gemset
ripgrep
skaffold
stylua
tesseract
the_silver_searcher
tika
tree
uv
vips
wget
yt-dlp
zoxide
zsh
zsh-completions
```

### Homebrew cask

現在 Homebrew cask で管理されているもの。

```text
anaconda
android-platform-tools
blackhole-16ch
blackhole-2ch
chromedriver
claude
codex-app
font-cica
font-noto-color-emoji
font-noto-emoji
font-zed-mono-nerd-font
gcloud-cli
ghostty
google-cloud-sdk
hashicorp-vagrant
rancher
temurin
utm
warp
zed
```

### `$HOME` 配下の手動導入ツール

現在 `$HOME` 配下に手動導入されているツール。

```text
~/.agent-tools/bin/agent-tools
~/.bun/bin/bun
~/.cargo/bin/cargo-add
~/.cargo/bin/cargo-audit
~/.cargo/bin/cargo-deny
~/.cargo/bin/cargo-llvm-cov
~/.cargo/bin/cargo-make
~/.cargo/bin/cargo-rm
~/.cargo/bin/cargo-set-version
~/.cargo/bin/cargo-spec
~/.cargo/bin/cargo-upgrade
~/.cargo/bin/cross
~/.cargo/bin/cross-util
~/.cargo/bin/diesel
~/.cargo/bin/makers
~/.cargo/bin/rust-script
~/.cargo/bin/rustup
~/.cargo/bin/skill-test
~/.cargo/bin/skill-tools
~/.local/bin/runxlrd.py
~/bin/phpactor
```

### Neovim が現在扱っている開発対象

Neovim 設定から、少なくとも以下を扱っている。

- Lua: `lua_ls`
- Rust: `rust_analyzer`
- Markdown: `marksman`, `markdown-preview.nvim`, `markdownlint`
- TypeScript / JavaScript: `tsserver`
- ReScript: `rescriptls`, `nvim-treesitter-rescript`
- Ruby: `ruby_lsp`
- Firestore: `vim-firestore`, `ftdetect/firestore.vim`
- Shell / zsh: dotfiles とテストスクリプト
- GitHub Copilot: `github/copilot.vim`

Homebrew と `$HOME` 配下ツールから、追加で以下の言語・実行環境を利用していると判断する。

- Node / Bun
- Python
- Rust
- Ruby
- Go
- PHP
- OCaml
- SQL / DB client: MySQL、PostgreSQL、SQLite 関連
- Cloud / Container: AWS CLI、Google Cloud SDK、Docker、docker compose、Colima、Rancher、Skaffold、kubectx

## 目標構成

最初の構成は host 固定にしない。

```text
flake.nix
home.nix
darwin.nix

modules/
  cli.nix
  languages.nix
  zsh.nix
  git.nix
  neovim.nix
  editor-apps.nix
  shell-files.nix
  app-configs.nix
  secrets.nix
  macos-defaults.nix
  launchagents.nix
  direnv.nix
  homebrew.nix
  macos.nix

templates/
  node/
  python/
  rust/
  ruby/
  go/
  php/
  ocaml/
  rescript/
  cloud/
```

`home.nix` が基本入口。

```sh
home-manager switch --flake .#ya
```

macOS の system defaults と cask / MAS / fonts は `darwin.nix` で管理し、`nixpkgs` の GUI は Home Manager 側（`copyApps` を含む）または nix-darwin 側（`/Applications/Nix Apps`）へ配置する。

```sh
sudo darwin-rebuild switch --flake .#ya
```

## レイヤー設計

### ownership ルール（重複禁止）

責務は次で固定する。

- Home Manager `programs.*` を優先する対象: 設定統合を持つ CLI。`git`、`gh`、`neovim`、`direnv` + `nix-direnv`、`zsh`、`fzf`、`atuin`、`zoxide`。
- Home Manager `home.packages` の対象: 設定統合をほぼ持たない単機能 binary（例: `jq`、`ripgrep`、`tree`、`ffmpeg`）。
- nix-darwin の対象: macOS defaults、`launchd`、fonts、Homebrew/MAS、system レベル設定。
- nix-homebrew の対象: Homebrew 本体と tap の所有権。`autoMigrate = true` で既存 install を取り込み、必要なら `mutableTaps = false` で tap の手動変更を禁止する。

同一ツールを `programs.*` と `home.packages` に二重定義しない。`programs.*` を使うツールは package 供給元を module 側へ寄せ、`home.packages` からは外す。

Homebrew package（formula/cask/MAS）の宣言は nix-darwin `homebrew.*` に集約し、Homebrew インストール本体と tap は nix-homebrew が所有する。`brew tap` を手動で増減させる運用は採らない。
nix-homebrew は nix-darwin module として適用する前提なので、導入順は「最小の nix-darwin 構成を先に有効化 -> nix-homebrew で Homebrew 本体/tap を所有 -> nix-darwin `homebrew.*` で formula/cask/MAS と macOS 側（fonts/defaults/launchd）を宣言管理」に固定する。

### Home Manager 運用モード（standalone / nix-darwin 統合）

この計画は次の 2 モードを許容する。どちらを選ぶかを README に明記し、混在運用しない。

1. standalone Home Manager:
`home-manager switch --flake .#ya` のみでユーザー環境を更新する。rollback 境界は Home Manager 世代。
2. nix-darwin 統合 Home Manager:
`sudo darwin-rebuild switch --flake .#ya` を唯一の適用コマンドにする。rollback 境界は nix-darwin system 世代（Home Manager 設定も同一世代で戻る）。

統合モード時は `darwin.nix` の Home Manager bridge で次を固定する。

- `home-manager.useGlobalPkgs = true`（nix-darwin と同じ `pkgs` を使う）
- `home-manager.useUserPackages = true`（ユーザー package 配置を HM に委譲する）

適用順序:

1. `nix flake check`
2. モードに応じた build（`home-manager build` または `darwin-rebuild build`）
3. モードに応じた switch（上記 2 コマンドのどちらか一方のみ）

### 多ユーザー運用

単一ユーザー入口 `.#ya` は初期移行の簡略化として扱う。複数ユーザーまたは複数 host へ広げる場合、flake 出力は次の境界で分離する。

```text
darwinConfigurations.<host>              system / host 設定
homeConfigurations."<user>@<host>"       standalone Home Manager 用 user 設定
darwinConfigurations.<host>.users.users  macOS user 定義
home-manager.users.<user>                nix-darwin 統合 Home Manager 用 user 設定
```

多ユーザー時の ownership:

- nix-darwin は host 共通の system 設定、Nix daemon、trusted users、Homebrew 本体/tap、fonts、launchd、macOS defaults を管理する。
- Home Manager は user ごとの shell、git、Neovim、XDG config、user package、user secret 復号先を管理する。
- Homebrew formula/cask/MAS は host 単位で重複管理しない。user ごとに異なる GUI が必要な場合は cask ではなく user-managed app か例外リストで扱う。
- `nix.settings.trusted-users` は最小にし、通常ユーザー全員を無条件に trusted user へ入れない。daemon 管理者だけを明示する。
- user ごとの mutable auth state（`~/.config/gcloud`、`~/.docker/config.json`、`~/.kube/config`、`~/.config/gh/hosts.yml` など）は共有しない。

多ユーザー用 module 分割:

```text
hosts/<host>/darwin.nix
users/<user>/home.nix
modules/darwin/*.nix
modules/home/*.nix
secrets/common.yaml
secrets/hosts/<host>.yaml
secrets/users/<user>.yaml
```

適用コマンド:

```sh
sudo darwin-rebuild switch --flake .#<host>
home-manager switch --flake '.#<user>@<host>'
```

統合 Home Manager を採る host では `sudo darwin-rebuild switch --flake .#<host>` だけを実行し、standalone `home-manager switch` と併用しない。

### Home Manager

Home Manager はユーザー環境を管理する。

- zsh
- git
- Neovim
- direnv / nix-direnv
- 共通 CLI
- グローバル言語環境
- formatter / linter
- XDG config
- dotfiles 配置
- 秘密情報を含まないアプリ設定
- shell 起動ファイル
- editor 設定

## 設定ファイル移行

既存 dotfiles だけでなく、現在 `$HOME`、`~/.config`、`~/Library/Preferences`、`~/Library/Application Support`、`~/Library/LaunchAgents` に存在する設定を移行対象として整理する。

設定はアプリ単位で次の 5 種類に分けて移行する。

```text
source config      人が編集する静的設定。Home Manager / nix-darwin へ移行する
secret config      静的な token、credential、秘密鍵。sops-nix へ移行する
service definition launchd / daemon 定義。nix-darwin launchd か service module へ移行する
runtime state      DB、cache、履歴、socket、pid、log、VM disk。移行作業で壊さないことを検証する
generated config   アプリが再生成する lock、backup、updater 設定。移行作業で上書きしないことを検証する
```

各アプリは「設定ファイルのパスを列挙する」だけでは不十分。主要アプリ節では source config を明記し、必要に応じて secret config、runtime state、service definition、検証コマンドを書く。該当しない分類は省略可とする。ただし runtime state / secret config / service definition が存在する場合は必ず明記する。generated config は必要なアプリだけ追加で書く。
CLI が自動更新する token DB、context、cache、login state は、秘密情報を含んでいても runtime state として扱う。sops-nix に入れるのは、人が管理し、CLI が通常運用で書き換えない静的 secret だけに限定する。

### secret manager 方針（sops-nix 採用）

この計画では secret manager を `sops-nix` に統一する。`agenix` と `sops-nix` を併記して別途設計にはしない。

- `agenix` は「単一または少数の age 暗号化ファイルを復号配置する」用途に適している。
- 今回 `sops-nix` を採る理由は、構造化 secret（YAML/JSON/.env）運用、複数 secret の分割管理、recipient/key backend の拡張余地、Home Manager / nix-darwin との統合を優先するため。
- runtime credential は `sops-nix` にも `agenix` にも入れない。例: Docker auth、OAuth token/session、CLI login state、macOS Keychain / 1Password 向け credential。

### 鍵管理

sops-nix 用の age key は、復号に必要な host / user だけへ配布する。private key は repository に commit しない。

推奨配置:

```text
/var/lib/sops-nix/key.txt        host secret 用。root:wheel、0400
~/.config/sops/age/keys.txt      user secret 用。0600
.sops.yaml                       recipient と secret file の対応を管理
secrets/common.yaml              全 host/user 共通の静的 secret
secrets/hosts/<host>.yaml        host 固有の静的 secret
secrets/users/<user>.yaml        user 固有の静的 secret
```

host secret と user secret の使い分け:

- nix-darwin activation、system daemon、launchd、Homebrew 管理に必要な secret は host secret として `/var/lib/sops-nix/key.txt` で復号する。
- zsh / git / cloud CLI など user 環境だけで使う secret は user secret として Home Manager 側で復号する。
- runtime credential は sops-nix に入れない。再ログインや CLI の refresh で更新される credential DB / token DB / context は mutable state として残す。
- SSH private key は、復号先 permission、ssh-agent / macOS Keychain 連携、backup、rotation 手順を確認できるものだけ移行する。

初期導入時の鍵投入:

```sh
sudo mkdir -p /var/lib/sops-nix
sudo install -m 0400 /path/to/sops-age-key.txt /var/lib/sops-nix/key.txt
mkdir -p ~/.config/sops/age
install -m 0600 /path/to/user-age-keys.txt ~/.config/sops/age/keys.txt
```

鍵更新:

```sh
sops updatekeys secrets/common.yaml
sops updatekeys secrets/hosts/<host>.yaml
sops updatekeys secrets/users/<user>.yaml
```

運用ルール:

- age private key は 1Password、暗号化外部媒体、または別管理の password manager で backup する。
- host を廃棄した場合は `.sops.yaml` から recipient を削除し、対象 secret に `sops updatekeys` を実行する。
- 漏えい時は新しい age key を作り、recipient 差し替え、`sops updatekeys`、旧 key 削除、復号確認を同一作業として実施する。
- CI / 無人適用では private key を長期常駐させず、必要な job scope だけで secret を供給する。

### zsh / Neovim の runtime/generated state 棚卸し

source config と分離して、次を runtime/generated state として扱う。

zsh:

```text
~/.zsh_history
~/.cache/zsh/.zcompdump-*
~/.cache/zsh/plugins.zsh（Antidote 運用時のみ）
```

Neovim:

```text
~/.local/share/nvim/site/pack/jetpack/*
~/.local/share/nvim/mason/*
~/.local/share/nvim/swap
~/.local/state/nvim/*
~/.cache/nvim/*
```

Phase 1（現行 Jetpack 維持）では `config/nvim/lua/omy/util.lua` の `curl -fsSLo ... vim-jetpack` bootstrap が初回ネットワーク依存を持つことを許容する。  
Phase 2 で plugin を Nix 管理へ移し、Jetpack bootstrap と plugin 更新のネットワーク依存を撤去する。

### 既に dotfiles にある設定

Home Manager で配置する。

```text
.zshrc                         -> home.file.".zshrc"
.gitconfig                     -> programs.git
config/zsh/.zshrc              -> xdg.configFile."zsh/.zshrc"
config/zsh/aliases.zsh         -> xdg.configFile."zsh/aliases.zsh"
config/zsh/completion.zsh      -> xdg.configFile."zsh/completion.zsh"
config/zsh/env.zsh             -> xdg.configFile."zsh/env.zsh"
config/zsh/history.zsh         -> xdg.configFile."zsh/history.zsh"
config/zsh/local.zsh           -> xdg.configFile."zsh/local.zsh"
config/zsh/options.zsh         -> xdg.configFile."zsh/options.zsh"
config/zsh/p10k.zsh            -> xdg.configFile."zsh/p10k.zsh"
config/zsh/plugins.txt         -> plugin 固定情報として Nix 側へ移行
config/zsh/plugins.zsh         -> Antidote 依存を削除し、Nix 管理 plugin 読み込みへ移行
config/zsh/prompt.zsh          -> xdg.configFile."zsh/prompt.zsh"
config/nvim/init.lua           -> xdg.configFile."nvim/init.lua"
config/nvim/lua/**             -> xdg.configFile."nvim/lua"
config/nvim/.stylua.toml       -> xdg.configFile."nvim/.stylua.toml"
```

`init.sh` は symlink 作成の役割を失う。移行後は削除し、初回導入手順は README に移す。

### 管理対象候補と除外方針を整理する設定

現在 dotfiles にない項目について、管理対象候補と除外方針を整理する。

```text
~/.config/atuin/config.toml       -> programs.atuin.settings
~/.config/colima/default/colima.yaml -> xdg.configFile."colima/default/colima.yaml"
~/.config/fish/config.fish        -> xdg.configFile."fish/config.fish"（既存ファイルをそのまま移す）
~/.config/git/ignore              -> programs.git.ignores
~/.config/gh/config.yml           -> xdg.configFile."gh/config.yml"
~/.config/glow/glow.yml           -> xdg.configFile."glow/glow.yml"
~/.config/helm/repositories.yaml  -> Home Manager で固定管理しない（`helm repo add/update` が更新する runtime/generated state）
~/.config/jj/config.toml          -> programs.jujutsu.settings
~/.config/karabiner/karabiner.json -> immutable 運用時のみ xdg.configFile."karabiner/karabiner.json"（mutable 運用なら固定管理しない）
~/.config/zed/settings.json       -> immutable 運用時のみ xdg.configFile."zed/settings.json"（mutable 運用なら固定管理しない）
~/.config/zed/keymap.json         -> immutable 運用時のみ xdg.configFile."zed/keymap.json"（mutable 運用なら固定管理しない）
~/Library/Application Support/Code/User/settings.json -> immutable 運用時のみ home.file（mutable 運用なら固定管理しない）
~/Library/Application Support/Code/User/keybindings.json -> immutable 運用時のみ home.file（mutable 運用なら固定管理しない）
~/Library/Application Support/com.mitchellh.ghostty/config -> home.file
~/.aws/config                     -> home.file.".aws/config"
~/.docker/config.json             -> Home Manager で丸ごとsymlink固定しない（runtime auth/stateを保持）
~/.kube/config                    -> Home Manager / sops-nix で丸ごとsymlink固定しない（runtime auth/stateを保持）
~/.gemrc                          -> home.file.".gemrc"
~/.ssh/config                     -> programs.ssh.matchBlocks
~/.serena/serena_config.yml       -> home.file.".serena/serena_config.yml"
~/.runpod/config.toml             -> 静的 secret だけ sops-nix 管理。CLI 更新 state は管理外
~/.gemini/settings.json           -> home.file.".gemini/settings.json"
~/.qwen/settings.json             -> home.file.".qwen/settings.json"
~/.claude.json                    -> auth/session を含む場合は固定管理しない。静的設定だけ分離できる場合のみ home.file
~/.copilot/mcp-config.json        -> 純粋な静的設定のみ home.file。Copilot auth/session/state は管理外
```

`home.file` / `xdg.configFile` は「人が編集する静的設定」だけに使う。CLI が通常運用で更新する token DB、credentials DB、hosts、session、context、cache は runtime auth state として扱い、symlink 固定しない。

### Colima 設定移行

Colima は `colima` binary だけを Nix に移しても移行完了ではない。  
次の単位で管理する。

source config:

```text
~/.config/colima/default/colima.yaml
```

現在の `default` profile 設定として固定する値:

```yaml
cpu: 5
disk: 300
memory: 6
arch: aarch64
runtime: docker
modelRunner: docker
kubernetes:
  enabled: false
  version: v1.35.0+k3s1
  k3sArgs:
    - --disable=traefik
  port: 0
autoActivate: true
network:
  address: false
  mode: shared
  interface: en0
  preferredRoute: false
  dns: null
  dnsHosts: {}
  hostAddresses: false
  gatewayAddress: 192.168.5.2
forwardAgent: false
docker: {}
vmType: vz
portForwarder: ssh
rosetta: false
binfmt: true
nestedVirtualization: false
mountType: virtiofs
mountInotify: true
cpuType: ""
```

service definition:

```text
~/Library/LaunchAgents/homebrew.mxcl.colima.plist
```

現在の LaunchAgent は `/opt/homebrew/opt/colima/bin/colima start -f` を実行している。  
移行後は Homebrew path を使わず、Nix 管理の `colima` を実行する launchd 定義に置き換える。既存の `homebrew.mxcl.colima` は `launchctl bootout` で停止し、Homebrew 管理 plist を削除してから Nix 側の agent を有効化する。Nix 側の label は Homebrew 由来でない名前にし、二重起動を避ける。

Docker context:

```text
colima -> unix:///Users/ya/.config/colima/default/docker.sock
default -> unix:///var/run/docker.sock
rancher-desktop -> unix:///Users/ya/.rd/docker.sock
```

移行後の Docker CLI は `colima` context を使えることを検証する。Rancher Desktop context は Rancher を併用する限り残す。

Colima 移行時に保全確認する runtime state:

```text
~/.config/colima/default/docker.sock
~/.config/colima/default/daemon
~/.config/colima/_lima
~/.config/colima/_store
~/.config/colima/.colima
~/.config/colima/ssh_config
```

Colima 必須検証:

```sh
command -v colima
colima version
docker context inspect colima --format '{{(index .Endpoints "docker").Host}}'
```

期待:

- `command -v colima` は Nix 由来。
- `default/colima.yaml` は Home Manager 管理。
- Colima の launchd 定義は `/opt/homebrew/opt/colima/bin/colima` を参照しない。
- Homebrew 由来の `homebrew.mxcl.colima` は unload 済みで、Nix 側の agent だけが有効。
- `docker context inspect colima` が `unix://$HOME/.config/colima/default/docker.sock` を返す。
- VM disk、pid、log、socket は Nix store に入らない。

### 主要アプリ別の設定移行単位

#### Docker CLI

source config:

```text
~/.docker/config.json の静的キー（例: `credsStore`、`credHelpers`、`plugins.cliPluginsExtraDirs`）
```

secret config:

```text
n/a（Docker login token は runtime auth state）
```

runtime state:

```text
~/.docker/config.json の `auths`、`currentContext`、token関連キー
~/.docker/.token_seed
~/.docker/buildx/*
```

service definition:

```text
n/a
```

`~/.docker/config.json` は Docker CLI が `docker login`、`docker context use`、credential helper 更新で書き換えるため、`home.file` で Nix store への symlink にしない。静的に固定したい値がある場合は、既存 JSON を保持したまま activation script で不足 key だけを merge するか、運用手順として `docker context use colima` を実行する。`auths`、token、buildx state は Nix / Home Manager / sops-nix の管理対象外にする。
Docker Desktop 由来の `credsStore: "desktop"` が残ると、Desktop 非依存運用（Colima + Nix Docker CLI）で credential helper 解決に失敗するため禁止する。`credsStore` / `credHelpers` の静的キー方針だけでなく、helper 名と `docker-credential-<helper>` 実バイナリの整合を検証する。

Compose v2:

```text
docker compose
```

`docker-compose` コマンドが存在するだけでは不十分。Docker CLI が Compose v2 plugin を認識することを検証する。Nix package の配置で `docker compose` が成立しない場合は、`cliPluginsExtraDirs` 相当の mutable-safe な設定か、Home Manager activation で `~/.docker/cli-plugins/docker-compose` へ symlink を作る。ただし `~/.docker/config.json` 全体は symlink 化しない。互換目的の `docker-compose` 単体 binary は成功条件にしない。

推奨実装は「Home Manager activation で `~/.docker/cli-plugins/docker-compose` を Nix store 実体へ張る」方式とする。`~/.docker/config.json` を symlink 化して plugin discovery を解決しない。

```text
~/.docker/cli-plugins/docker-compose -> /nix/store/...-docker-compose-.../libexec/docker/cli-plugins/docker-compose（実体）
```

Compose plugin の供給元は Nix 由来のみを許容する。plugin path または `~/.docker/cli-plugins/docker-compose` symlink の詳細な実体確認は、将来 `scripts/verify-nix-migration.sh` に切り出す。
`docker-compose` は Compose 互換コマンドとしても Nix 由来のみ許容し、Homebrew 由来バイナリ残存を許可しない。

検証は `### Container / Docker / Colima` の共通ブロックで実施する。

#### Google Cloud SDK

source config:

```text
~/.config/gcloud/configurations/config_default
~/.config/gcloud/configurations/config_nhk
```

runtime state:

```text
~/.config/gcloud/active_config
~/.config/gcloud/access_tokens.db
~/.config/gcloud/credentials.db
~/.config/gcloud/legacy_credentials
~/.config/gcloud/default_configs.db
~/.config/gcloud/logs
~/.config/gcloud/cache
~/.config/gcloud/virtenv
~/.config/gcloud/application_default_credentials.json（ADC）
```

secret config:

```text
n/a（通常の user login / ADC は runtime auth state）
```

service definition:

```text
n/a
```

移行方針:

- configuration は公開可能な project / account 名だけを Home Manager 管理に移す。`active_config` は `gcloud config configurations activate` が更新するため、Home Manager では固定管理せず mutable state として残す。
- credentials と token DB は sops-nix で管理しない。`gcloud auth login` / token refresh が更新する runtime state として保全する。
- 静的な service account key を使う場合だけ sops-nix 管理にする。
- `application_default_credentials.json` は ADC の runtime auth state として扱い、固定管理しない。ADC が必要な project は `gcloud auth application-default login` の再実行手順として README に記録する。
- GKE 利用時の追加 component は `pkgs.google-cloud-sdk.withExtraComponents (with pkgs.google-cloud-sdk.components; [ gke-gcloud-auth-plugin ])` で宣言管理する。
- `gcloud components install ...` は Nix 管理 SDK を mutable に壊すため禁止する。必要 component は Nix 側へ追加し、`gcloud components` での手動変更を運用手順に含めない。

検証:

```sh
gcloud config configurations list
gcloud config list
gcloud auth list
```

#### AWS

source config:

```text
~/.aws/config
~/.aws/config 内の `credential_process` 設定（実行可能ヘルパーを使う場合）
```

secret config:

```text
~/.aws/credentials.d/static（長期 credential を使う場合のみ。sops-nix の復号先）
```

runtime state:

```text
~/.aws/credentials（CLI が更新する一時 credential を置く）
~/.aws/sso/cache
~/.aws/cli/cache
```

service definition:

```text
n/a
```

移行方針:

- region、output、profile 名は Home Manager 管理。
- `~/.aws/credentials` は固定管理しない。SSO/MFA/STS の cache / session は runtime auth state として扱う。
- `credential_process` を使う場合は「実行可能ファイルが AWS CLI 規定の JSON を stdout に返す」ヘルパーだけを使う。`~/.aws/credentials.d/static` のような平文ファイルを `credential_process` で直接読む設計にはしない。
- 長期 static credential は sops-nix で `~/.aws/credentials.d/static` に復号し、必要時のみ読み込む。常用の `~/.aws/credentials` へ恒久的に同居させない。
- 実行時は `AWS_SHARED_CREDENTIALS_FILE="$HOME/.aws/credentials.d/static" aws ... --profile static-admin` のようにコマンド単位で credential file を切り替えるか、手動/ヘルパーで一時的に `~/.aws/credentials` へ取り込んで利用後に除去する。
- `~/.aws/config` で `source_profile` に `~/.aws/credentials.d/static` 前提を持ち込まない。`default` や運用 profile に長期 secret と一時 credential を同居させない。

検証:

```sh
aws configure list-profiles
aws configure list
aws sts get-caller-identity
test -f ~/.aws/credentials.d/static
test -r ~/.aws/credentials.d/static
ls -l ~/.aws/credentials.d/static
AWS_SHARED_CREDENTIALS_FILE="$HOME/.aws/credentials.d/static" aws configure list --profile static-admin
```

#### Kubernetes

source config:

```text
必要な場合のみ、静的 kubeconfig 断片（例: `~/.kube/config.static`）
```

secret config:

```text
必要な場合のみ、静的 service account token / client cert（通常運用では n/a）
```

runtime state:

```text
~/.kube/config
~/.kube/kubens/rancher-desktop
```

service definition:

```text
n/a
```

移行方針:

- `~/.kube/config` 全体は sops-nix で固定管理しない。Rancher Desktop / Colima / gcloud が context、cluster、user を追加・更新するため mutable state として保全する。
- 静的な kubeconfig 断片や service account token が必要な場合だけ sops-nix 管理にし、`KUBECONFIG` で mutable な `~/.kube/config` と合成する。
- Rancher Desktop / Colima / gcloud が生成する context は再生成可能な runtime state として扱う。
- `kubectl` は cloud 操作と zsh prompt（`p10k`）で常用するため、devShell 専用にせず Home Manager 側でグローバル供給する。

検証:

```sh
command -v kubectl
kubectl config get-contexts
kubectl config current-context
kubectx
```

#### SSH

source config:

```text
~/.ssh/config
```

secret config:

```text
~/.ssh/id_*
~/.ssh/*.pem
```

runtime state:

```text
~/.ssh/known_hosts
~/.ssh/google_compute_known_hosts
```

service definition:

```text
n/a
```

移行方針:

- Host、User、IdentityFile、ForwardAgent などの接続定義は `programs.ssh.matchBlocks` へ移す。
- 秘密鍵は、復号先の permission と macOS keychain / ssh-agent 運用を確認できるものだけ sops-nix で管理する。既存鍵を移す前に backup と rollback 手順を用意する。
- known_hosts は状態として扱う。

検証:

```sh
ssh -G github.com >/dev/null
ssh -G github.com | rg -n '^(identityfile|identitiesonly|ignoreunknown) '
```

期待:

- `ssh -G github.com` が 0 で終了し、`programs.ssh.matchBlocks` の設定が評価される。
- agent 非依存運用では `ssh -G` 出力で `IdentityFile` / `IdentitiesOnly` / `IgnoreUnknown` の宣言を確認できればよく、`ssh-add -l` は必須ではない。

`ssh-agent` 運用時の追加確認:

```sh
ssh-add -l
```

- `ssh-agent` を使う運用では、`ssh-add -l` が `Could not open a connection to your authentication agent.` を返した場合は失敗にする。
- 鍵未ロードを許容しない運用では `ssh-add -l` の鍵一覧表示を必須にする。
- 鍵未ロードを許容する運用では `ssh-add -l` が `The agent has no identities.` を返しても失敗にしない。

#### Atuin

source config:

```text
~/.config/atuin/config.toml
```

secret config:

```text
~/.local/share/atuin/key（運用で回転しない場合のみ）
```

runtime state:

```text
~/.local/share/atuin/history.db
~/.local/share/atuin/session
```

service definition:

```text
n/a
```

移行方針:

- UI、search、sync 設定は `programs.atuin.settings`。
- history DB は Home Manager の配置対象にせず、永続 state として保全する。key は Atuin sync 用の静的 secret として扱い、運用で回転しない場合のみ sops-nix 管理に移す。session は再ログインで再生成される runtime auth state として扱う。

検証:

```sh
atuin doctor
atuin stats
```

#### GitHub CLI / Copilot

source config:

```text
~/.config/gh/config.yml
~/.copilot/mcp-config.json（標準パス）
```

secret config:

```text
n/a（認証情報は runtime auth state として扱う）
```

runtime state:

```text
~/.config/gh/hosts.yml
~/.config/github-copilot/hosts.json
~/.config/.copilot/config.json
~/.config/.copilot/logs
~/.config/.copilot/mcp-config.json（旧パス。見つかった場合は `~/.copilot/mcp-config.json` へ統合）
```

service definition:

```text
n/a
```

移行方針:

- 表示・protocol・editor などの gh 設定は Home Manager 管理。
- `hosts.yml`、Copilot auth、token cache は CLI / plugin が更新する auth state として扱い、sops-nix で固定管理しない。必要なら `gh auth login` と Copilot login の再実行手順を README に記録する。
- logs は state として保全する。

検証:

```sh
gh auth status
gh config list
gh extension list | rg '^github/gh-copilot$'
gh copilot status
test ! -L ~/.config/gh/hosts.yml
test ! -L ~/.config/github-copilot/hosts.json
```

#### Zed

source config:

```text
~/.config/zed/settings.json
~/.config/zed/keymap.json
~/Library/Preferences/dev.zed.Zed.plist
```

runtime state:

```text
~/.config/zed/conversations
~/.config/zed/embeddings
~/.config/zed/prompts
~/Library/Application Support/Zed/languages
~/Library/Application Support/Zed/prettier
~/Library/Application Support/Zed/extensions
~/Library/Application Support/Zed/hang_traces
```

移行方針:

- 当面は mutable 運用を標準にし、`settings.json` / `keymap.json` は固定管理しない。再現性を優先する場合のみ immutable 運用として Home Manager 管理に切り替える。
- LSP は Zed の管理に任せ、Nix は runtime / formatter / linter 供給を優先する。
- conversations、embeddings、extensions、downloaded language servers は state として保全する。

検証:

```sh
ls -l ~/.config/zed/settings.json
ls -l ~/.config/zed/keymap.json
```

期待:

- `settings.json` と `keymap.json` は immutable 運用なら両方 symlink、mutable 運用なら両方通常ファイル。
- 片方だけ symlink の状態は失敗として扱う。

#### VS Code

source config:

```text
~/Library/Application Support/Code/User/settings.json
~/Library/Application Support/Code/User/keybindings.json
```

runtime state:

```text
~/Library/Application Support/Code/User/History
~/Library/Application Support/Code/User/globalStorage
~/Library/Application Support/Code/User/workspaceStorage
~/Library/Application Support/Code/User/sync
```

移行方針:

- 当面は mutable 運用を標準にし、User settings と keybindings は固定管理しない。再現性を優先する場合のみ immutable 運用として Home Manager 管理に切り替える。
- History、workspaceStorage、globalStorage、sync cache は state として保全する。

検証:

```sh
ls -l "$HOME/Library/Application Support/Code/User/settings.json"
ls -l "$HOME/Library/Application Support/Code/User/keybindings.json"
```

期待:

- `settings.json` と `keybindings.json` は immutable 運用なら両方 symlink、mutable 運用なら両方通常ファイル。
- 片方だけ symlink の状態は失敗として扱う。

#### Karabiner-Elements

source config:

```text
~/.config/karabiner/karabiner.json
~/.config/karabiner/assets/complex_modifications/gistfile1.txt
```

runtime state:

```text
~/.config/karabiner/automatic_backups
```

移行方針:

- 当面は mutable 運用を標準にし、`karabiner.json` は固定管理しない。UI 編集を禁止できる場合のみ immutable 運用として Home Manager 管理に切り替える。
- automatic backups は state として保全する。

検証:

```sh
ls -l ~/.config/karabiner/karabiner.json
```

期待:

- immutable 運用では symlink、mutable 運用では通常ファイル。

#### Ghostty / iTerm2 / Warp

source config:

```text
~/Library/Application Support/com.mitchellh.ghostty/config
~/Library/Preferences/com.googlecode.iterm2.plist
~/Library/Preferences/dev.warp.Warp-Stable.plist
```

secret config:

```text
n/a
```

runtime state:

```text
window state、recent files、update state（各アプリが生成する可変データ）
```

service definition:

```text
n/a
```

移行方針:

- Ghostty の text config は Home Manager 管理。
- iTerm2 と Warp の plist は、再現したい key だけ defaults / plist 管理に移す。
- window state、recent files、update state は runtime state として保全する。

検証:

```sh
test -L "$HOME/Library/Application Support/com.mitchellh.ghostty/config"
```

### macOS Preferences と defaults

`~/Library/Preferences` は全体を Nix store にコピーしない。  
再現したい設定値だけを nix-darwin の `system.defaults.CustomUserPreferences` に写す。

nix-darwin で管理する macOS defaults:

```text
~/Library/Preferences/.GlobalPreferences.plist
~/Library/Preferences/com.apple.dock.plist
~/Library/Preferences/com.apple.finder.plist
~/Library/Preferences/com.apple.screencapture.plist
~/Library/Preferences/com.apple.symbolichotkeys.plist
~/Library/Preferences/com.apple.controlcenter.plist
~/Library/Preferences/com.apple.trackpad / mouse / keyboard 関連 plist
~/Library/Preferences/com.apple.HIToolbox.plist
~/Library/Preferences/com.apple.TextInputMenu.plist
~/Library/Preferences/com.apple.inputmethod.Kotoeri.plist
~/Library/Preferences/com.apple.Terminal.plist
```

アプリ設定として管理する Preferences:

```text
~/Library/Preferences/com.googlecode.iterm2.plist
~/Library/Preferences/com.mitchellh.ghostty.plist
~/Library/Preferences/dev.zed.Zed.plist
~/Library/Preferences/dev.warp.Warp-Stable.plist
~/Library/Preferences/io.rancherdesktop.app.plist
~/Library/Preferences/rancher-desktop/settings.json
~/Library/Preferences/com.microsoft.VSCode.plist
~/Library/Preferences/org.pqrs.Karabiner-Elements.Settings.plist
~/Library/Preferences/org.pqrs.Karabiner-Elements.Preferences.plist
~/Library/Preferences/org.videolan.vlc.plist
~/Library/Preferences/org.mozilla.firefox.plist
~/Library/Preferences/org.mozilla.thunderbird.plist
```

アプリ本体の宣言管理に従属させる Preferences:

```text
~/Library/Preferences/ByHost/*
~/Library/Preferences/*ShipIt*.plist
~/Library/Preferences/*Sparkle*.plist
~/Library/Preferences/*AutoUpdater*.plist
~/Library/Preferences/*notbackedup*.plist
~/Library/Preferences/com.apple.AuthKit*.plist
~/Library/Preferences/com.apple.account*.plist
~/Library/Preferences/com.apple.icloud*.plist
~/Library/Preferences/com.apple.identityservicesd.plist
~/Library/Preferences/com.apple.ids*.plist
~/Library/Preferences/com.apple.imservice*.plist
~/Library/Preferences/com.apple.Safari*.plist
~/Library/Preferences/com.apple.Maps*.plist
~/Library/Preferences/com.apple.MobileSMS*.plist
~/Library/Preferences/com.apple.Passwords.plist
~/Library/Preferences/com.apple.security*.plist
~/Library/Preferences/com.apple.wifi*.plist
~/Library/Preferences/com.google.Keystone.Agent.plist
~/Library/Preferences/us.zoom.*.plist
```

### LaunchAgents

`~/Library/LaunchAgents` は launchd 定義として移行対象に含める。

アプリ本体の宣言管理に従属させる LaunchAgents:

```text
~/Library/LaunchAgents/com.google.GoogleUpdater.wake.plist -> google-chrome / google-drive cask 管理と整合させる
~/Library/LaunchAgents/com.google.keystone.agent.plist -> google-chrome / google-drive cask 管理と整合させる
~/Library/LaunchAgents/com.google.keystone.xpcservice.plist -> google-chrome / google-drive cask 管理と整合させる
~/Library/LaunchAgents/com.valvesoftware.steamclean.plist -> Steam app 管理と整合させる
```

Nix 側で定義する LaunchAgent:

```text
~/Library/LaunchAgents/homebrew.mxcl.colima.plist -> 停止・削除し、Nix 側の `org.nix.colima` launchd.user.agents へ移行
```

### 削除・統合する設定

Nix 移行後は不要にする。

```text
~/.zprofile        -> pipx / cargo PATH 追加を削除し、ログイン shell 用の設定は Home Manager に統合
~/.zshenv          -> 削除
~/.bash_profile    -> 削除
~/.bashrc          -> 削除
~/.profile         -> 削除
~/.cshrc           -> 削除
~/.tcshrc          -> 削除
~/.mkshrc          -> 削除
~/.vimrc           -> Neovim へ統合。OPAM が追記した Vim 設定は Nix 管理の OCaml 環境へ移行
~/.yarnrc          -> 削除し、Node/Corepack 管理へ移行
~/.ocamlinit       -> OCaml devShell / global OCaml 設定へ統合
~/.boto            -> 静的 secret だけ sops-nix 管理。gcloud / boto が更新する state は管理外
```

### サブプロジェクトとして扱う設定

`~/.agent-tools` は単なる設定ディレクトリではなく、`Cargo.toml`、`deny.toml`、`docker-compose.test.yaml`、`release-please-config.json` を持つ独立したプロジェクトとして扱う。

```text
~/.agent-tools/config.yaml
~/.agent-tools/settings.json
~/.agent-tools/deny.toml
~/.agent-tools/docker-compose.test.yaml
~/.agent-tools/release-please-config.json
```

dotfiles に取り込むのではなく、`~/.agent-tools` 側で Nix 化する。

### 設定ファイル検証

```sh
home-manager build --flake .#ya
home-manager switch --flake .#ya
ls -l ~/.zshrc ~/.gitconfig ~/.gemrc ~/.ssh/config
ls -l ~/.config/zsh ~/.config/nvim ~/.config/atuin ~/.config/jj ~/.config/glow ~/.config/zed ~/.config/karabiner
ls -l ~/.aws/config ~/.docker/config.json ~/.config/gh/config.yml
ls -l "$HOME/Library/Application Support/Code/User/settings.json"
ls -l "$HOME/Library/Application Support/com.mitchellh.ghostty/config"
find "$HOME" -maxdepth 1 -mindepth 1 -print 2>/dev/null | sort || true
find "$HOME/.config" -type f -print 2>/dev/null | sort || true
find "$HOME/Library/Preferences" -maxdepth 2 -type f -print 2>/dev/null | sort || true
find "$HOME/Library/LaunchAgents" -maxdepth 2 -type f -print 2>/dev/null | sort || true
```

期待:

- Home Manager 管理ファイルは Nix store 由来の symlink になる。
- 秘密情報を含むファイルは Nix store に入らない。
- アプリの状態ディレクトリや cache は Home Manager によって上書きされない。
- `init.sh` が作った symlink と Home Manager の配置が衝突しない。
- `~/.ssh/id_*`、`~/.aws/credentials` は静的 secret と runtime auth state に分けて扱われている。
- `~/.config/gcloud/credentials.db`、`~/.config/gh/hosts.yml` は sops-nix で固定管理せず、再ログインで復元する認証状態として扱われている。
- `~/Library/Preferences` は、nix-darwin defaults で管理する plist とアプリ管理に従属する plist に分かれている。
- `~/Library/LaunchAgents/homebrew.mxcl.colima.plist` は停止・削除され、Nix 側の launchd 管理へ移行されている。

### nix-darwin

nix-darwin はこの移行で導入し、macOS 側の管理を担当する。

- Homebrew cask
- Homebrew tap
- Mac App Store apps
- fonts
- macOS defaults
- shell registration
- launchd service

### devShell

devShell はプロジェクト固有環境を管理する。

- グローバルと異なる runtime version
- project 固有の runtime / compiler / formatter / linter
- formatter
- linter
- DB client
- cloud CLI
- emulator
- code generator

## グローバル CLI

Home Manager で常用 CLI を管理する。  
ただし「設定統合があるツール」は `programs.*` を優先し、`home.packages` は単機能 binary のみを置く。

`programs.*` で管理する対象:

```nix
programs.git.enable = true;
programs.gh.enable = true;
programs.neovim.enable = true;
programs.direnv = {
  enable = true;
  nix-direnv.enable = true;
};
programs.zsh.enable = true;
programs.fzf = {
  enable = true;
  enableZshIntegration = false;
};
programs.atuin.enable = true;
programs.zoxide.enable = true;
```

fzf 連携は `programs.fzf.enable = true; enableZshIntegration = false;` を固定し、fzf の Zsh 統合一括有効化（`completion.zsh` / `fzf --zsh`）は使わない。`config/zsh/completion.zsh` では Nix 由来 `key-bindings.zsh` のみを手動 source し、`^I` は classic completion（`expand-or-complete`）、`^X^I` は `fzf-tab-complete` に固定する。

Home Manager で管理する packages:

```nix
home.packages = with pkgs; [
  automake
  android-tools
  awscli2
  chromedriver
  cmake
  colima
  coreutils
  docker
  docker-buildx
  docker-compose
  docker-credential-helpers
  fd
  ffmpeg
  (pkgs.google-cloud-sdk.withExtraComponents (with pkgs.google-cloud-sdk.components; [ gke-gcloud-auth-plugin ]))
  glow
  graphviz
  helix
  jq
  kubectl
  jujutsu
  kubectx
  mas
  mysql-client
  postgresql_14
  sqlite-interactive
  temurin-bin
  ripgrep
  skaffold
  stylua
  tesseract
  tree
  uv
  vips
  wget
  yt-dlp
  zsh-completions
];
```

実装時には、選択した nixpkgs revision で attr 名を必ず確認する。
Compose v2 は Nix package `docker-compose` を Docker CLI plugin payload として供給し、検証は `docker compose version` を成功条件にする。`docker-compose` 単体 binary の存在確認は成功条件にしない。
`docker-credential-helper`（Homebrew formula 名）は Nix attr `docker-credential-helpers` へ移す。
DB client は `mysql` / `psql` / `sqlite3` を Nix 由来に統一する。SQLite CLI は `sqlite-interactive` に統一する。
`colima` binary は Home Manager `home.packages` で供給し、常駐起動は nix-darwin `launchd.user.agents` で管理する。

## グローバル言語環境

言語環境はグローバルで使えるようにする。  
プロジェクトごとの固定は devShell で上書きする。

### Node / JavaScript / TypeScript / Bun

現在の問題:

- `nodebrew` が Homebrew 管理。
- `~/.bun/bin` は PATH 汚染確認と手動導入 inventory の対象で、Nix 優先 PATH と競合する。
- `docs/zsh-key-operations-full.md` に、nodebrew 配下の Node から来た `rg` が記録されている。

移行先:

```nix
home.packages = with pkgs; [
  nodejs_22
  corepack
  bun
  nodePackages.typescript
  nodePackages.prettier
  nodePackages.eslint
  nodePackages.markdownlint-cli
];
```

廃止対象:

```text
nodebrew
~/.nodebrew/current/bin
~/.bun/bin
```

検証:

```sh
command -v node
command -v npm
command -v corepack
command -v bun
node --version
bun --version
```

期待:

- すべて Nix 由来の path を指す。
- nodebrew と `~/.bun/bin` を経由しない。

### Python

現在の問題:

- Homebrew に `python@3.9`、`python@3.10`、`python@3.11` がある。
- `pyenv` と `pipx` が Homebrew 管理。
- README が pyenv と `pip install neovim` を案内している。

移行先:

```nix
home.packages = with pkgs; [
  uv
  pyright
  ruff
  black
  (python311.withPackages (ps: with ps; [
    pip
    virtualenv
    ipython
    pytest
    pynvim
    requests
  ]))
];
```

廃止対象:

```text
pyenv
pipx
python@3.9
python@3.10
python@3.11
~/.local/bin にある Python CLI の無管理状態
```

検証:

```sh
command -v python
command -v pip
command -v uv
command -v pyright
command -v ruff
command -v black
python --version
python -c 'import pynvim'
```

期待:

- Python は Nix 由来。
- pyenv と Homebrew Python を経由しない。
- Neovim Python provider が成立する。

### Rust

現在の問題:

- `~/.cargo/bin` に cargo install 由来の CLI が多数ある。
- `rustup` も `~/.cargo/bin` にある。
- Neovim は `rust_analyzer` を Mason で入れる設定になっている。

移行先:

グローバル用途では Nix から固定された Rust toolchain と周辺 CLI を提供する。`rustup` は toolchain を `~/.rustup` に mutable に展開するため、再現性と rollback を優先するグローバル環境では使わない。

```nix
home.packages = with pkgs; [
  rustc
  cargo
  clippy
  rustfmt
  cargo-audit
  cargo-deny
  cargo-edit
  cargo-llvm-cov
  cargo-make
  diesel-cli
];
```

厳密な toolchain 固定が必要なプロジェクトでは、devShell で `fenix` または `rust-overlay` を使う。

置換:

```text
cargo-add/cargo-rm/cargo-upgrade -> cargo-edit
cargo-audit -> cargo-audit
cargo-deny -> cargo-deny
cargo-llvm-cov -> cargo-llvm-cov
cargo-make/makers -> cargo-make
diesel -> diesel-cli
rustup -> グローバルでは廃止し、必要な project だけ devShell で明示
```

移行判定が必要な CLI:

```text
cargo-set-version
cargo-spec
cross
cross-util
rust-script
skill-test
skill-tools
```

この CLI 群は `~/.cargo/bin` を PATH から外す前に、次の 3 択で必ず決定する。

- Nix package へ置換する。
- project devShell に閉じ込める。
- 例外リストに残し、用途・見直し条件・削除期限を README に記録する。

検証:

```sh
command -v rustc
command -v cargo
command -v cargo-audit
command -v cargo-deny
command -v cargo-make
command -v diesel
rustc --version
cargo --version
```

期待:

- 管理対象 CLI は Nix 由来。
- `~/.cargo/bin` に依存しない。

### Ruby

現在の問題:

- `rbenv-gemset` が Homebrew 管理。
- Neovim は `ruby_lsp` を使う設定がある。

移行先:

```nix
home.packages = with pkgs; [
  ruby_3_3
  rubyPackages_3_3.bundler
];
```

廃止対象:

```text
rbenv-gemset
rbenv 前提の PATH
```

検証:

```sh
command -v ruby
command -v bundle
ruby --version
bundle --version
```

### Go

現在の問題:

- `go` が Homebrew 管理。
- Go 関連の LSP / debugger は明示管理されていない。

移行先:

```nix
home.packages = with pkgs; [
  go_1_23
  golangci-lint
  delve
];
```

検証:

```sh
command -v go
command -v golangci-lint
command -v dlv
go version
```

### PHP

現在の問題:

- `composer` は Homebrew 管理。
- `~/bin/phpactor` が手動導入。

移行先:

```nix
home.packages = with pkgs; [
  php
  composer
];
```

廃止対象:

```text
~/bin/phpactor
```

検証:

```sh
command -v php
command -v composer
php -v
composer --version
```

### OCaml

現在の問題:

- `opam` が Homebrew 管理。

移行先:

```nix
home.packages = with pkgs; [
  ocaml
  dune_3
  opam
  ocamlPackages.utop
];
```

移行期は Nix から `opam` を提供する。OCaml project を devShell 化した後に `opam` のグローバル提供を削除し、グローバルは `ocaml` / `dune` / `utop` に寄せる。

検証:

```sh
command -v ocaml
command -v dune
command -v opam
command -v utop
ocaml -version
dune --version
```

### ReScript

現在の状況:

- Neovim に `rescriptls` 設定がある。
- `nvim-treesitter-rescript` を使っている。

移行方針:

- ReScript compiler は Nix package を優先する。LSP はエディタ側管理を優先する。
- nixpkgs に十分なものがなければ、Node/Bun 側の package として devShell に閉じ込める。
- Neovim の `rescriptls` は editor 管理の LSP（Mason 等）または PATH 上の LSP を参照する。

検証:

```sh
command -v rescript
```

ReScript project では次も実行する:

```sh
rescript build
```

### Lua / Neovim Lua

現在の状況:

- Neovim 設定は Lua で書かれている。
- `lua_ls` を Mason で入れる設定がある。
- `stylua` は Homebrew 管理。

移行先:

```nix
home.packages = with pkgs; [
  stylua
];
```

検証:

```sh
command -v stylua
nvim --headless '+lua print(vim.fn.exepath("stylua"))' '+qa'
```

### Markdown / 文書

現在の状況:

- `marksman` が Homebrew 管理。
- Neovim は `markdown-preview.nvim` を使う。
- null-ls で `markdownlint` を使っている。
- この移行計画書を含む docs は Markdown。

移行先:

```nix
home.packages = with pkgs; [
  nodePackages.markdownlint-cli
  glow
];
```

検証:

```sh
command -v markdownlint
command -v glow
```

## Neovim 移行

現在の問題:

- `mason-lspconfig` が `lua_ls`、`rust_analyzer`、`marksman`、`tsserver` を `ensure_installed` している。
- `mason-null-ls` が `stylua` を `ensure_installed` している。
- LSP を Nix の source of truth に寄せると、他エディタ/エージェントと方針不一致になる。

移行先:

- Neovim 本体は Nix。
- plugin 管理は Phase 1 では既存 Jetpack を維持し、Phase 2 で Nix 管理へ移す。
- LSP は Neovim 側管理を優先し、Mason installer は利用可とする。
- Nix は language runtime / compiler / formatter / linter / CLI / build tool の供給を担当する。
- 「Neovim だけ LSP を Nix 管理」に寄せる中途半端な運用は採らない。

Nix で供給するもの:

```text
pyright
ruff
black
stylua
markdownlint-cli
prettier
eslint
```

Neovim 側の変更:

- LSP installer（`mason-lspconfig`）は維持可能。
- Mason は `require("mason").setup({ PATH = "skip" })` を必須にし、Mason bin の PATH 先頭挿入を禁止する（Nix 供給の formatter/linter を常に優先するため）。
- formatter / linter は Nix を source of truth に固定するため、`mason-null-ls` の installer から `stylua` 等を外す（`ensure_installed = {}` または該当 plugin 無効化）。
- formatter / linter の Mason 残骸は検出だけでなく purge する。最低限 `~/.local/share/nvim/mason/bin/stylua`、`~/.local/share/nvim/mason/bin/markdownlint`、`~/.local/share/nvim/mason/packages/{stylua,markdownlint}` を削除し、再インストール対象から外す。
- `lspconfig` は Mason 管理 LSP または PATH 上 executable のどちらでも動く状態にする。
- TypeScript LSP は repo 現状では `config/nvim/lua/omy/configs/mason-lspconfig.lua` の `ensure_installed` にのみ `tsserver` があるため、ここを `ts_ls` に更新する。`config/nvim/lua/omy/configs/lspconfig.lua` には現状 TS エントリがないので、custom settings が必要な場合にだけ `ts_ls` override を追加する。

検証:

```sh
nvim --headless '+checkhealth' '+qa'
nvim --headless '+lua print(vim.fn.exists(":Mason"))' '+qa'
nvim --headless '+lua print(vim.fn.exepath("stylua"))' '+qa'
nvim --headless '+lua print(vim.fn.exepath("markdownlint"))' '+qa'
nvim --headless '+lua print(vim.fn.exepath("stylua"):match("/nvim/mason/") and "mason" or "non-mason")' '+qa'
nvim --headless '+lua print(vim.fn.exepath("markdownlint"):match("/nvim/mason/") and "mason" or "non-mason")' '+qa'
rg -n 'PATH\\s*=\\s*"skip"' config/nvim/lua/omy/configs
test ! -x ~/.local/share/nvim/mason/bin/stylua
test ! -x ~/.local/share/nvim/mason/bin/markdownlint
! find ~/.local/share/nvim/mason/packages -maxdepth 1 -type d 2>/dev/null | rg '/(stylua|markdownlint)$'
rg -n 'tsserver' config/nvim/lua
rg -n 'ts_ls' config/nvim/lua
```

Mason 依存確認:

```sh
find ~/.local/share/nvim/mason -maxdepth 3 -type f 2>/dev/null | sort || true
```

期待:

- LSP は Neovim 管理（Mason 含む）で運用できる。
- Mason は PATH 先頭挿入を行わない（`PATH = "skip"`）。
- formatter / linter は Nix 由来を優先し、Mason installer の対象にしない。
- `tsserver` は `mason-lspconfig` の `ensure_installed` から `ts_ls` へ移行済み。
- `lspconfig.lua` に `ts_ls` override を置くのは custom settings が必要な場合のみ。
- Mason 配下が存在しても失敗条件にしない。

## zsh 移行

現在の問題:

- `config/zsh/plugins.zsh` が Antidote を起動時に clone する可能性がある。
- `config/zsh/plugins.txt` は Antidote 時代の固定リストで、Home Manager plugin 定義と二重管理になる。
- `config/zsh/env.zsh` が Homebrew、Rancher Desktop、pipx、nodebrew、agent-tools の PATH を追加している。
- `.zshrc` は現在 Rancher Desktop の PATH を追加している。Bun 手動 PATH は `.zshrc` ではなく PATH 汚染 inventory で扱う。
- `config/zsh/completion.zsh` が `/opt/homebrew/opt/fzf/shell/key-bindings.zsh` / `/usr/local/opt/fzf/shell/key-bindings.zsh` を直接 source している。

移行先:

- zsh 本体は Nix。
- plugin は Home Manager で固定する。
- PATH は Nix を優先し、手動導入系の優先 PATH を消す。
- fzf key-bindings は Homebrew path source を廃止し、`programs.fzf.enable = true; enableZshIntegration = false;` を前提に Nix 由来 `key-bindings.zsh` のみを手動 source する。`completion.zsh` と `fzf --zsh` は使わず、`^I` は classic completion（`expand-or-complete`）、`^X^I` は `fzf-tab-complete` に固定する。

Home Manager での plugin 定義は src/file/order を固定する。`powerlevel10k` はテーマ本体と `config/zsh/p10k.zsh` を組で読み込む。

```nix
programs.zsh.plugins = [
  {
    name = "powerlevel10k";
    src = pkgs.zsh-powerlevel10k;
    file = "share/zsh-powerlevel10k/powerlevel10k.zsh-theme";
  }
  {
    name = "fzf-tab";
    src = pkgs.zsh-fzf-tab;
    file = "share/fzf-tab/fzf-tab.plugin.zsh";
  }
  {
    name = "zsh-autosuggestions";
    src = pkgs.zsh-autosuggestions;
    file = "share/zsh-autosuggestions/zsh-autosuggestions.zsh";
  }
  {
    name = "fast-syntax-highlighting";
    src = pkgs.zsh-fast-syntax-highlighting;
    file = "share/zsh/site-functions/fast-syntax-highlighting.plugin.zsh";
  }
];
```

`programs.zsh.enableCompletion = true` で `compinit` を先に実行し、その後の plugin 読み込み順序を `powerlevel10k -> fzf-tab -> zsh-autosuggestions -> fast-syntax-highlighting -> ~/.config/zsh/p10k.zsh` に固定する。`fzf-tab` は `compinit` 後かつ autosuggestions/syntax-highlighting より前に置く。続けて `config/zsh/completion.zsh` で `bindkey '^I' expand-or-complete` と `bindkey '^X^I' fzf-tab-complete` を設定する。

`programs.fzf.enableZshIntegration` は `false` に固定する。fzf 側の Zsh 統合は一括 source せず、`config/zsh/completion.zsh` で `${pkgs.fzf}/share/fzf/key-bindings.zsh` だけを手動 source する。`completion.zsh` と `fzf --zsh` は読み込まない。

`enableCompletion` は `compinit` 実行と `nix-zsh-completions` 連携が中心で、third-party の `pkgs.zsh-completions` は自動導入されない前提で扱う。Homebrew 置換は保守側へ倒し、`home.packages` へ `pkgs.zsh-completions` を明示追加し、`$fpath` へ組み込む責務を Home Manager 側に持たせる。

管理対象 plugin:

```text
powerlevel10k
zsh-autosuggestions
fast-syntax-highlighting
fzf-tab
```

補完定義として管理するもの:

```text
zsh-completions
```

削除する PATH:

```text
/opt/homebrew/bin の無条件優先
/usr/local/bin の無条件優先
~/.nodebrew/current/bin
~/.bun/bin
~/.cargo/bin
~/.pyenv/bin
~/.rbenv/bin
~/.local/bin の無条件優先
```

Nix 管理外として残す PATH:

```text
~/.agent-tools/bin
~/.rd/bin
```

Nix 管理外である理由を README に書く。
Rancher Desktop が追加する `### MANAGED BY RANCHER DESKTOP ...` の shell injection block は ランタイム/アプリ管理状態 として扱い、repo 管理の `.zshrc` / Home Manager 管理 shell 設定へ固定しない。必要なら Rancher Desktop 側で再生成する。

zsh テストスクリプト移行タスク:

- `scripts/test-zsh-shortcuts.sh` は `config/zsh/plugins.txt` 前提チェックを削除し、Home Manager plugin 定義経由で widget が有効化されること（`fzf-tab-complete`、`autosuggest-accept`、highlight 関数）を検証する。
- `scripts/test-zsh-shortcuts.sh` の Homebrew 前提（`/opt/homebrew` / `/usr/local` の fzf key-bindings や補完参照）を削除し、Nix 由来または Home Manager 統合有効化を成功条件にする。
- `scripts/test-zsh-key-operations-full.sh` は `config/zsh/plugins.txt` と Antidote 生成物（`~/.cache/zsh/plugins.zsh`）依存を削除し、未存在でも通ることを成功条件にする。
- `scripts/test-zsh-key-operations-full.sh` は PATH 判定を更新し、`~/.nodebrew/current/bin`、`~/.bun/bin`、`~/.cargo/bin`、`~/.pyenv/bin`、`~/.rbenv/bin` の優先出現を失敗条件にする一方、`~/.agent-tools/bin` と `~/.rd/bin` は許容する。
- 期待する移行後挙動は「`config/zsh/plugins.txt` がなくても両スクリプトが成功し、Homebrew 由来 key-bindings/path を参照せず、Nix 管理 plugin と PATH 判定で pass する」こととする。

検証:

```sh
zsh -i -c 'echo ok'
zsh -i -c 'print -l $path'
zsh -i -c 'command -v fzf atuin zoxide'
zsh -i -c 'autoload -Uz compinit; compinit; for d in $fpath; do [[ -f "$d/_vnstat" ]] && print -r -- "$d/_vnstat" && exit 0; done; exit 1'
zsh -i -c 'autoload -Uz compinit; compinit; whence -w _vnstat | rg "_vnstat: function$"'
zsh -i -c '(( $+widgets[fzf-tab-complete] ))'
zsh -i -c '(( $+widgets[autosuggest-accept] ))'
zsh -i -c 'typeset -f _zsh_highlight >/dev/null || typeset -f _fast_highlight >/dev/null'
! rg -n '/opt/homebrew/opt/fzf/shell/key-bindings.zsh|/usr/local/opt/fzf/shell/key-bindings.zsh' config/zsh/completion.zsh
! awk 'BEGIN{bad=0} /^[[:space:]]*#/{next} /completion[.]zsh|fzf[[:space:]]+--zsh/{print; bad=1} END{exit bad}' config/zsh/completion.zsh
nix path-info -r .#homeConfigurations.ya.activationPackage | rg '/zsh-completions-'
nix path-info -r ~/.local/state/nix/profiles/home-manager | rg '/zsh-completions-'
ZSH_COMPLETIONS=$(nix build --no-link --print-out-paths nixpkgs#zsh-completions) && test -f "$ZSH_COMPLETIONS/share/zsh/site-functions/_vnstat"
bash scripts/test-zsh-shortcuts.sh
bash scripts/test-zsh-key-operations-full.sh
```

期待:

- Nix 管理 plugin が読み込まれる。
- `zsh-completions` は Homebrew formula ではなく `pkgs.zsh-completions` を供給元にし、plugin source ではなく `$fpath` と `compinit` の対象として扱われる。
- Home Manager の package closure/profile に `zsh-completions` が存在する。
- `compinit` 後に `pkgs.zsh-completions` 由来の既知 completion file（例: `_vnstat`）が `$fpath` 経由で解決でき、`whence -w _vnstat` が `function` 解決になる（generic な `site-functions` 文字列検出だけで通過させない）。
- `fzf-tab`、`zsh-autosuggestions`、syntax highlighting の zle widget / 関数が存在する。
- Homebrew 由来の `key-bindings.zsh` source は残っていない。
- `scripts/test-zsh-shortcuts.sh` と `scripts/test-zsh-key-operations-full.sh` は `config/zsh/plugins.txt` 非依存で成功する。
- 起動時 clone は発生しない。

## Git 移行

現在の `.gitconfig` は GitHub credential helper に Homebrew の `gh` を直接参照している。

```text
!/opt/homebrew/bin/gh auth git-credential
```

移行後は Nix 由来の `gh` を使う。

既存 `.gitconfig` の設定は credential helper だけでなく全項目を移す。現時点で確認済みの `init.defaultBranch`、`alias.graph`、`credential.*` を全て Home Manager へ移行し、今後項目が増えた場合も「`.gitconfig 全量を programs.git 配下へ反映」を原則にする。

Home Manager で管理する Git 設定:

```nix
programs.git = {
  enable = true;
  aliases = {
    graph = "log --graph --date-order -C -M --pretty=format:\\\"<%h> %ad [%an] %Cgreen%d%Creset %s\\\" --all --date=short";
  };
  extraConfig = {
    init.defaultBranch = "main";
    credential."https://github.com".helper = [ "" "!gh auth git-credential" ];
    credential."https://gist.github.com".helper = [ "" "!gh auth git-credential" ];
  };
};
```

`helper =` のリセット行を先頭に置き、続けて `helper = !gh auth git-credential` を置く順序を保持する。これにより既存 `.gitconfig` と同じ解決順で GitHub/Gist の helper を上書きできる。

検証:

```sh
git config --global --get init.defaultBranch
git config --global --get alias.graph
git config --global --get-all credential.https://github.com.helper
git config --global --get-all credential.https://gist.github.com.helper
command -v gh
gh auth status
```

## Homebrew 移行分類

### Nix へ移す CLI / formula / SDK

```text
atuin
automake
awscli
cmake
colima
composer
coreutils
docker
docker-buildx
docker-compose
docker-credential-helper
ffmpeg
fzf
gh
git
glow
go
gobject-introspection
graphviz
helix
jj
jpeg
jq
kubectx
libass
libffi
make
mysql-client
neovim
opam
postgresql@14
ripgrep
skaffold
stylua
tesseract
the_silver_searcher
tika
tree
uv
vips
wget
yt-dlp
zoxide
zsh
zsh-completions
mysql (client binary は mysql-client として Nix 供給)
openvino
```

### Nix へ移す cask 起点 CLI / SDK

```text
android-platform-tools
chromedriver
gcloud-cli
google-cloud-sdk
temurin
hashicorp-vagrant
```

CLI / SDK / daemon / 開発ツールは原則として Nix package へ移す。`GUI だから Homebrew` ではなく、判定順は `Nix で問題なく運用できるなら Nix` -> `MAS が正規配布経路なら MAS` -> `署名/TCC/自動更新/ライセンス等で Nix 運用が破綻する場合のみ cask または手動例外` とする。`gcloud-cli` と `google-cloud-sdk` のように同系統のものがある場合は Nix の `google-cloud-sdk` だけを採用し、PATH 上に複数 provider を残さない。
nix-darwin の `homebrew.casks` には `gcloud-cli` / `google-cloud-sdk` を入れない。
`hashicorp-vagrant` のような dev tool は GUI 例外へ混ぜず、Nix package / devShell / 明示例外のいずれかで扱う。

主要な名前対応は次で統一する。

```text
android-platform-tools (brew cask) -> android-tools (nix)
awscli (brew) -> awscli2 (nix)
chromedriver (brew cask) -> chromedriver (nix)
jj (brew) -> jujutsu (nix attr, binary は jj)
go (brew) -> go_1_23 (nix)
temurin (brew) -> temurin-bin (nix)
gcloud-cli (brew cask) -> google-cloud-sdk (nix)
google-cloud-sdk (brew cask) -> google-cloud-sdk (nix)
docker-compose (brew formula) -> docker-compose (nix attr, Compose v2 plugin payload)
docker-credential-helper (brew formula 名) -> docker-credential-helpers (nix attr 名)
hashicorp-vagrant (brew cask) -> vagrant (nix)
```

### Homebrew 移行先トレーサビリティ

`Nix へ移す` に列挙した項目の移行先は次で固定する。`nix search` / `nix repl` / `nix eval` で attr 実在確認後に実装する。

| Homebrew 項目 | 移行先 | 実装先 | 備考 |
| --- | --- | --- | --- |
| `atuin` | Home Manager module | Home Manager `programs.atuin` | package は module 側に委譲（重複定義しない） |
| `automake` | Nix package | Home Manager `home.packages` | `pkgs.automake` |
| `awscli` | Nix package | Home Manager `home.packages` | `pkgs.awscli2` |
| `cmake` | Nix package | Home Manager `home.packages` | `pkgs.cmake` |
| `colima` | Nix package + launchd | Home Manager `home.packages` + nix-darwin `launchd.user.agents` | binary と常駐起動を分離管理 |
| `composer` | Nix package | Home Manager `home.packages` | `pkgs.php84Packages.composer` 想定 |
| `coreutils` | Nix package | Home Manager `home.packages` | `pkgs.coreutils` |
| `docker` | Nix package | Home Manager `home.packages` | `pkgs.docker` |
| `docker-buildx` | Nix package | Home Manager `home.packages` | `pkgs.docker-buildx` |
| `docker-compose` | Nix package | Home Manager `home.packages` | `pkgs.docker-compose` を Compose v2 plugin payload として使用 |
| `docker-credential-helper` | Nix package | Home Manager `home.packages` | Nix attr は `docker-credential-helpers` |
| `ffmpeg` | Nix package | Home Manager `home.packages` | `pkgs.ffmpeg` |
| `fzf` | Home Manager module | Home Manager `programs.fzf` + `programs.zsh.initExtra` | `programs.fzf.enable = true; enableZshIntegration = false;` を固定し、`${pkgs.fzf}/share/fzf/key-bindings.zsh` のみ手動 source（`completion.zsh`/`fzf --zsh` は不使用、`^I` は `expand-or-complete`、`^X^I` は `fzf-tab-complete`） |
| `gh` | Home Manager module | Home Manager `programs.gh` | package は module 側に委譲（重複定義しない） |
| `git` | Home Manager module | Home Manager `programs.git` | `.gitconfig` 全設定を `programs.git` へ移行 |
| `glow` | Nix package | Home Manager `home.packages` | `pkgs.glow` |
| `go` | Nix package | Home Manager `home.packages` | `pkgs.go_1_23` 想定 |
| `gobject-introspection` | Nix package | devShell | プロジェクト依存の build input として扱う |
| `graphviz` | Nix package | Home Manager `home.packages` | `pkgs.graphviz` |
| `helix` | Nix package | Home Manager `home.packages` | `pkgs.helix` |
| `jj` | Nix package | Home Manager `home.packages` | Nix attr は `jujutsu` |
| `jpeg` | Nix package | devShell | ライブラリ依存として扱い、単独グローバル導入しない |
| `jq` | Nix package | Home Manager `home.packages` | `pkgs.jq` |
| `kubectl`（現状は cloud 用） | Nix package | Home Manager `home.packages` | `pkgs.kubectl`（Kubernetes 検証と prompt 依存のためグローバル供給） |
| `kubectx` | Nix package | Home Manager `home.packages` | `pkgs.kubectx` |
| `libass` | Nix package | devShell | ライブラリ依存として扱う |
| `libffi` | Nix package | devShell | ライブラリ依存として扱う |
| `make` | Nix package | Home Manager `home.packages` | `pkgs.gnumake` |
| `marksman` | Homebrew 廃止（editor 管理へ移管） | Neovim / Zed 設定（Mason 等） | Homebrew / Nix を LSP 供給元にしない |
| `mysql-client` | Nix package（client のみ） | Home Manager `home.packages` | `pkgs.mysql-client` |
| `mysql` | Nix package（client のみ） | Home Manager `home.packages` | `mysql` command は `pkgs.mysql-client` で供給し、MySQL server はグローバル導入しない（必要時 devShell / service / container で別管理） |
| `neovim` | Home Manager module | Home Manager `programs.neovim` | package は module 側に委譲（重複定義しない） |
| `opam` | Nix package | Home Manager `home.packages` | `pkgs.opam` |
| `postgresql@14` | Nix package | Home Manager `home.packages` | `pkgs.postgresql_14` |
| `SQLite`（macOS標準/手動導入分を置換） | Nix package | Home Manager `home.packages` | `pkgs.sqlite-interactive` |
| `ripgrep` | Nix package | Home Manager `home.packages` | `pkgs.ripgrep` |
| `skaffold` | Nix package | Home Manager `home.packages` | `pkgs.skaffold` |
| `stylua` | Nix package | Home Manager `home.packages` | `pkgs.stylua` |
| `tesseract` | Nix package | Home Manager `home.packages` | `pkgs.tesseract` |
| `the_silver_searcher` | Nix package | Home Manager `home.packages` | `pkgs.silver-searcher` |
| `tika` | Nix package | devShell | `pkgs.apache-tika` 想定（project 限定） |
| `tree` | Nix package | Home Manager `home.packages` | `pkgs.tree` |
| `uv` | Nix package | Home Manager `home.packages` | `pkgs.uv` |
| `vips` | Nix package | Home Manager `home.packages` | `pkgs.vips` |
| `wget` | Nix package | Home Manager `home.packages` | `pkgs.wget` |
| `yt-dlp` | Nix package | Home Manager `home.packages` | `pkgs.yt-dlp` |
| `zoxide` | Home Manager module | Home Manager `programs.zoxide` | package は module 側に委譲（重複定義しない） |
| `zsh` | Home Manager module | Home Manager `programs.zsh` | plugin/src/file/order を Home Manager で固定 |
| `zsh-completions` | Nix package + fpath 管理 | Home Manager `home.packages` + `programs.zsh` | `pkgs.zsh-completions` を明示導入し、`$fpath` へ追加して `compinit` 対象にする（brew formula は残さない） |
| `openvino` | Nix package / 手動SDK例外 / 導入保留 | devShell（優先）/ project-local container / 手動SDK例外 | Homebrew は供給元にしない。`pkgs.openvino` が適用困難な間は `homebrew.casks`/formula に宣言せず、移行先確定まで導入保留 |
| `android-platform-tools` | Nix package | Home Manager `home.packages` | Nix attr は `android-tools` |
| `chromedriver` | Nix package | Home Manager `home.packages` | `pkgs.chromedriver` |
| `gcloud-cli` | Nix package | Home Manager `home.packages` | `pkgs.google-cloud-sdk` へ統一 |
| `google-cloud-sdk` | Nix package | Home Manager `home.packages` | `pkgs.google-cloud-sdk.withExtraComponents (with pkgs.google-cloud-sdk.components; [ gke-gcloud-auth-plugin ])` を優先し、`gcloud components install` は使わない |
| `temurin` | Nix package | Home Manager `home.packages` | Nix attr は `temurin-bin` |
| `hashicorp-vagrant` | Nix package / devShell / 条件付き例外 | Home Manager `home.packages`（優先）/ devShell / cask例外 | dev tool 扱い。cask 維持は例外票必須 |

フォント cask の移行先は次で固定する。

| Homebrew 項目 | 移行先 | 実装先 |
| --- | --- | --- |
| `font-noto-color-emoji` | Nix package | nix-darwin `fonts.packages`（`noto-fonts-color-emoji`） |
| `font-noto-emoji` | Nix package | nix-darwin `fonts.packages`（`noto-fonts-emoji`） |
| `font-zed-mono-nerd-font` | Nix package | nix-darwin `fonts.packages`（`nerd-fonts.zed-mono`） |
| `font-cica` | Homebrew 残留 | nix-darwin `homebrew.casks`（代替 attr 未確定の間のみ） |

### Homebrew 残存禁止カテゴリ

Nix 移行方針との整合のため、次は Homebrew を供給元として残さない。

- dev tool / CLI / daemon
- language runtime / compiler
- LSP 実行バイナリと editor 連携用 toolchain（LSP 管理主体は editor 側）
- formatter / linter / build tool

### 廃止する formula

```text
marksman
nodebrew
pyenv
pipx
python@3.9
python@3.10
python@3.11
rbenv-gemset
```

### 廃止前に依存確認する formula

```text
openssl@1.1
```

`openssl@1.1` は直接使う CLI ではなく互換用ライブラリなので、Homebrew 側の依存関係を確認してから削除する。

検証:

```sh
brew uses --installed openssl@1.1
brew list --versions openssl@1.1
```

削除手順:

```sh
brew uninstall openssl@1.1
```

期待:

- `brew uses --installed openssl@1.1` が空である。
- `openssl@1.1` を削除しても依存関係エラーが出ない。

## cask / GUI アプリ

CLI / daemon / 開発ツールは GUI 例外に混ぜず、Nix（必要時 devShell）で管理する。GUI アプリは次の判定順で管理する。

1. Nix で署名・実行・更新運用まで問題なく回るものは Nix 管理。
2. Mac App Store が正規経路のものは `masApps` で管理。
3. Nix で署名/TCC/更新/ライセンス等が破綻するものだけ cask または手動を例外採用。

GUI アプリ配置の境界は次で固定する。

- Home Manager 管理: `home.stateVersion < 25.11` は `~/Applications/Home Manager Apps` への symlink 配置、`home.stateVersion >= 25.11` は `copyApps` による `~/Applications/Home Manager Apps` への copy 配置（default）で運用する。
- nix-darwin 管理: `/Applications/Nix Apps` 配下への配置で運用するもの。
- cask / MAS: 署名、TCC、store 連携、auto-update 制約で Nix 配置が破綻するもの。

`/Applications/Foo.app` 直置きを必須とするアプリは、原則 `cask` / `MAS` / 手動導入、または nix-darwin の custom activation で対応する。nixpkgs GUI をそのまま `/Applications` 直下へ置く前提では設計しない。

「nixpkgs の GUI アプリは全部 Home Manager へ入れる」は採らない。配置要件（`~/Applications` か `/Applications` か）で実装先を分ける。

cask 例外は「一時避難」扱いとし、次を必須にする。

- 例外理由（何が Nix で破綻するか）
- 識別子（cask 名 / bundle id / app 名）
- 残存検証コマンド
- 再評価条件（期限または upstream 追従条件）

`homebrew.casks` の宣言対象は、この必須項目を満たした例外台帳エントリのみとする。名称列挙だけの項目は「候補」に留め、宣言しない。

`anaconda` は dev tool / runtime 供給元のため GUI 例外にしない。`homebrew.casks` には宣言せず、Nix / devShell / プロジェクトローカル環境で管理する。

Homebrew cask 宣言例外台帳（nix-darwin `homebrew.casks` に宣言してよいもの）:

| cask 名 | 例外理由 | 識別子 | 残存検証 | 再評価条件 |
| --- | --- | --- | --- | --- |
| `font-cica` | `nixpkgs` 側に同等品質の安定 attr が未確定 | cask: `font-cica` | `brew list --cask | rg '^font-cica$'` と `fc-list | rg -i 'cica'` | `nixpkgs` に代替 attr が安定提供された時点、または四半期レビュー時 |

既存 Homebrew cask の候補（台帳化完了まで宣言しない）:

```text
blackhole-16ch
blackhole-2ch
claude
codex-app
ghostty
rancher
utm
warp
zed
```

fonts 境界は次で固定する。

- `fonts.packages` へ移すもの（Nix 再現可能）:

```text
font-noto-color-emoji -> noto-fonts-color-emoji
font-noto-emoji -> noto-fonts-emoji
font-zed-mono-nerd-font -> nerd-fonts.zed-mono（利用中のnixpkgs attrを実装時に確認）
```

- Homebrew cask に残すもの（残す理由つき）:

```text
font-cica（同等パッケージの再現性確認が必要。nixpkgsに安定attrがなければcask維持）
```

追加の Homebrew cask 候補（理由・識別子・検証・再評価条件を台帳化後にのみ宣言）:

```text
alacritty
blender
deepl
discord
firefox
google-chrome
google-drive
karabiner-elements
libreoffice
macdroid
microsoft-office
microsoft-teams
nanoem
notion
skype
slack
thunderbird
vlc
visual-studio-code
windows-app
zulip
iterm2
zoom
```

Mac App Store 管理へ移すアプリ:

```text
GarageBand
Keynote
Numbers
Pages
iMovie
Xcode
LINE
Amazon Kindle
```

`mas` コマンドの供給元は Nix package `mas` に固定し、Homebrew formula / cask では管理しない。

`homebrew.masApps` から削除したアプリは `brew cleanup` だけでは自動アンインストールされないことがある。まず `mas list` と宣言の差分をレビューし、不要 ID が確定した場合にだけ手動 remediation を行う。
`homebrew.masApps` は nix-darwin activation 時に `brew bundle` 経由で適用されるため、install / update / uninstall のいずれも `mas` 側の実装や配布形態によっては権限昇格（`sudo` / root）を要求する場合がある。CI/無人適用前に対話要否を確認する。

`masApps` は app 名ではなく ID が必須なので、ID 未確定のアプリは宣言しない。次を実装前必須手順にする。

```sh
command -v mas
realpath "$(command -v mas)"
mas version
mas list
nix eval .#darwinConfigurations.ya.config.homebrew.masApps --json | jq -r 'to_entries[] | .value | if type == "object" and has("id") then .id else . end' | sort -u
comm -3 <(mas list | awk '{print $1}' | sort -u) <(nix eval .#darwinConfigurations.ya.config.homebrew.masApps --json | jq -r 'to_entries[] | .value | if type == "object" and has("id") then .id else . end' | sort -u)
mas search 'GarageBand'
mas search 'Keynote'
mas search 'Numbers'
mas search 'Pages'
mas search 'iMovie'
mas search 'Xcode'
mas search 'LINE'
mas search 'Amazon Kindle'
```

判定:

- `masApps` に載せるのは ID が取得できたものだけ。
- `command -v mas` と `realpath "$(command -v mas)"` は `/nix/store` または Nix profile（例: `/etc/profiles/per-user/...`、`/nix/var/nix/profiles/...`）由来である。
- `comm -3 ...` で差分が出る間は `homebrew.masApps` に宣言しない。ID 収集完了後にだけ宣言する。
- ID が不明なものはこの計画書では候補扱いに留め、宣言対象にしない。

差分レビュー後の手動 remediation（routine 検証コマンドに含めない）:

- `comm -3 ...` の差分から不要 ID が確定した場合にのみ `mas uninstall <APP_ID>` を実行する。
- `mas uninstall` は destructive 操作なので定常検証には使わない。
- `mas` の version / 配布形態によっては install / update / uninstall の各操作で `sudo` あるいは root 権限が必要になる場合がある。

Nix 管理対象外として残すアプリ:

```text
Safari
Steam
Factorio Demo
Getting Over It with Bennett Foddy
Minecraft
Vampire Survivors
Authenticator
JW Library
JustJoin
Google Docs
Google Sheets
Google Slides
uTorrent Web
個別ライセンスや手動更新が必要なアプリ
```

## devShell templates

グローバル言語環境は維持する。  
devShell はプロジェクトが異なるバージョンやツールを要求する場合に使う。

### Node

```nix
pkgs.mkShell {
  packages = with pkgs; [
    nodejs_22
    corepack
    bun
    nodePackages.typescript
    nodePackages.prettier
    nodePackages.eslint
  ];
}
```

### Python

```nix
pkgs.mkShell {
  packages = [
    pkgs.uv
    pkgs.pyright
    pkgs.ruff
    pkgs.black
    (pkgs.python311.withPackages (ps: with ps; [
      pip
      virtualenv
      ipython
      pytest
    ]))
  ];
}
```

### Rust

```nix
pkgs.mkShell {
  packages = with pkgs; [
    rustc
    cargo
    clippy
    rustfmt
    cargo-audit
    cargo-deny
    cargo-edit
    cargo-llvm-cov
    cargo-make
  ];
}
```

### Ruby

```nix
pkgs.mkShell {
  packages = with pkgs; [
    ruby_3_3
    rubyPackages_3_3.bundler
  ];
}
```

### Go

```nix
pkgs.mkShell {
  packages = with pkgs; [
    go_1_23
    golangci-lint
    delve
  ];
}
```

### PHP

```nix
pkgs.mkShell {
  packages = with pkgs; [
    php
    composer
  ];
}
```

### OCaml

```nix
pkgs.mkShell {
  packages = with pkgs; [
    ocaml
    dune_3
    opam
    ocamlPackages.utop
  ];
}
```

### Cloud / Container

```nix
pkgs.mkShell {
  packages = with pkgs; [
    awscli2
    google-cloud-sdk
    kubectl
    kubectx
    skaffold
    docker
    docker-compose
  ];
}
```

Cloud / Container devShell は CLI client だけを提供する。Docker daemon、Colima VM、socket、context、credential store は devShell の責務にしない。`docker compose` は Docker CLI plugin として認識されることを検証する。

## 検証計画

検証は「ビルドできる」「コマンドが存在する」だけでは不十分。  
以下を分けて確認する。

- Nix 評価が通ること。
- 適用前に差分を確認できること。
- 適用後にコマンドの解決元が Nix になっていること。
- 旧 Homebrew / 手動インストール由来のコマンドが優先されていないこと。
- 常駐系ツールは daemon / VM / socket まで動作確認すること。
- Neovim は起動だけでなく、Mason と formatter/linter 実行バイナリの解決状態まで確認すること。
- 削除作業は削除前後で差分確認すること。
- rollback が実際に実行可能であること。

検証コマンドは原則ワンライナーで記述し、複雑な判定は「期待:」に分離する。将来、詳細自動検証が必要になった場合は `scripts/verify-nix-migration.sh` に切り出す。

### 事前スナップショット

移行前の状態を保存して、削除・置換の根拠にする。

```sh
brew list --formula | sort
brew list --cask | sort
find ~/.cargo/bin ~/.bun/bin ~/.local/bin ~/bin ~/.nodebrew ~/.pyenv ~/.rbenv -maxdepth 2 -type f 2>/dev/null | sort || true
zsh -i -c 'print -l $path'
zsh -i -c 'for c in git gh node python ruby cargo bun nvim docker colima; do printf "%s\t%s\n" "$c" "$(command -v "$c" 2>/dev/null || true)"; done'
```

期待:

- 移行前の Homebrew formula / cask / 手動導入物 / PATH / command 解決元が記録できる。
- 棚卸し用途なので、`command -v` で未導入の項目は空表示でよい。
- 以後の削除対象はこの記録と Nix 置換結果を照合して決める。

### Flake / Home Manager

```sh
nix flake check
nix eval .#homeConfigurations.ya.activationPackage.drvPath
home-manager build --flake .#ya
```

CI で実行する候補（ワンライナー）:

```sh
nix flake check
home-manager build --flake .#ya
sudo darwin-rebuild build --flake .#ya
```

適用前に activation package の build が通ることを確認する。  
`switch` は build と差分確認後に実行する。

```sh
home-manager switch --flake .#ya
```

期待:

- 評価と build が通る。
- activation で既存ファイル衝突が起きない。
- `init.sh` の symlink と Home Manager 管理先の衝突を解消してから進める。

### nix-darwin

nix-darwin の build と switch を確認する。

```sh
sudo darwin-rebuild build --flake .#ya
sudo darwin-rebuild switch --flake .#ya
```

期待:

- Homebrew cask / fonts / macOS 設定が宣言通りに評価される。
- 既存アプリの上書きや削除対象を適用前に確認する。

### zsh

```sh
zsh -i -c 'echo ok'
zsh -i -c 'print -l $path'
zsh -i -c 'command -v git gh jq rg fzf atuin zoxide nvim'
zsh -i -c 'for c in git gh jq rg fzf atuin zoxide nvim; do printf "%s\t%s\n" "$c" "$(command -v "$c")"; done'
zsh -i -c 'autoload -Uz compinit; compinit; for d in $fpath; do [[ -f "$d/_vnstat" ]] && print -r -- "$d/_vnstat" && exit 0; done; exit 1'
zsh -i -c 'autoload -Uz compinit; compinit; whence -w _vnstat | rg "_vnstat: function$"'
zsh -i -c '(( $+widgets[fzf-tab-complete] ))'
zsh -i -c '(( $+widgets[autosuggest-accept] ))'
zsh -i -c 'typeset -f _zsh_highlight >/dev/null || typeset -f _fast_highlight >/dev/null'
nix path-info -r .#homeConfigurations.ya.activationPackage | rg '/zsh-completions-'
nix path-info -r ~/.local/state/nix/profiles/home-manager | rg '/zsh-completions-'
ZSH_COMPLETIONS=$(nix build --no-link --print-out-paths nixpkgs#zsh-completions) && test -f "$ZSH_COMPLETIONS/share/zsh/site-functions/_vnstat"
bash scripts/test-zsh-shortcuts.sh
bash scripts/test-zsh-key-operations-full.sh
```

期待:

- zsh がエラーなく起動する。
- Nix 管理の CLI が解決される。
- Homebrew CLI が Nix より優先されない。
- `~/.nodebrew/current/bin`、`~/.bun/bin`、`~/.cargo/bin`、`~/.pyenv/bin`、`~/.rbenv/bin` が優先 PATH に残らない。
- zsh plugin が起動時 clone ではなく Nix 管理の source / fpath から読み込まれる。
- `scripts/test-zsh-shortcuts.sh` と `scripts/test-zsh-key-operations-full.sh` は Antidote 時代の `config/zsh/plugins.txt` や Homebrew 前提に依存せず成功する。

### グローバル言語環境

```sh
zsh -i -c 'command -v node npm corepack bun'
zsh -i -c 'command -v python pip uv pyright ruff black'
zsh -i -c 'command -v rustc cargo cargo-audit cargo-deny cargo-make'
zsh -i -c 'command -v ruby bundle'
zsh -i -c 'command -v go golangci-lint'
zsh -i -c 'command -v php composer'
zsh -i -c 'command -v ocaml dune utop'
zsh -i -c 'for c in node python ruby cargo bun; do printf "%s\t%s\n" "$c" "$(command -v "$c")"; done'
```

期待:

- 主要言語 CLI はすべてグローバルで使える（`opam` は移行期のみ任意）。
- 表示された主要 CLI の解決先は Nix 由来で、`/opt/homebrew` や `/usr/local` を指さない。
- `nodebrew`、`pyenv`、`rbenv`、`pipx`、`~/.cargo/bin`、`~/.bun/bin` に依存しない。

### PATH 汚染確認

```sh
zsh -i -c 'print -l $path'
! zsh -i -c 'print -l $path' | rg -n 'nodebrew|pyenv|rbenv|pipx|/\\.cargo/bin|/\\.bun/bin'
zsh -i -c 'command -v node python ruby cargo bun'
```

期待:

- 旧手動導入 path が優先 PATH に残らない。
- `~/.cargo/bin` / `~/.bun/bin`（実際の表示では `$HOME/.cargo/bin` / `$HOME/.bun/bin`）が PATH 汚染として検出対象になる。
- 管理対象コマンドは Nix 由来で、Homebrew / 手動導入 path を優先しない。

### Neovim

```sh
nvim --headless '+checkhealth' '+qa'
nvim --headless '+lua print(vim.fn.exists(":Mason"))' '+qa'
nvim --headless '+lua print(vim.fn.exepath("stylua"))' '+qa'
nvim --headless '+lua print(vim.fn.exepath("markdownlint"))' '+qa'
nvim --headless '+lua print(vim.fn.exists(":Copilot"))' '+qa'
```

Mason 依存確認:

```sh
find ~/.local/share/nvim/mason -maxdepth 3 -type f 2>/dev/null | sort || true
```

期待:

- LSP は editor 管理（Mason 含む）を許容し、Mason 配下の存在を失敗条件にしない。
- formatter / linter は Nix 由来。devShell 内では active devShell 由来。

### Container / Docker / Colima

CLI provider 検証（daemon 起動不要）:

```sh
test ! -L ~/.docker/config.json
command -v docker
realpath "$(command -v docker)"
docker --version
docker compose version
docker --help | rg -n 'compose'
test -L ~/.docker/cli-plugins/docker-compose
realpath ~/.docker/cli-plugins/docker-compose
docker context ls
jq -r '.credsStore? // empty, (.credHelpers? // {} | to_entries[] | .value)' ~/.docker/config.json | sort -u
if jq -r '.credsStore? // empty, (.credHelpers? // {} | to_entries[] | .value)' ~/.docker/config.json | sort -u | rg -qx 'desktop'; then false; fi
jq -r '.credsStore? // empty, (.credHelpers? // {} | to_entries[] | .value)' ~/.docker/config.json | sort -u | awk 'NF' | xargs -I{} sh -c 'command -v docker-credential-{} >/dev/null'
```

Colima runtime 検証（Colima を使う環境で実行）:

```sh
command -v colima
colima version
colima status
docker context inspect colima --format '{{(index .Endpoints "docker").Host}}'
docker --context colima info
docker --context colima run --rm hello-world
docker buildx ls
launchctl print "gui/$(id -u)/org.nix.colima"
```

期待:

- `colima`、`docker`、`docker compose` が Nix 由来。
- `docker compose` が plugin として動作し、`docker-compose` 単体の存在確認だけに依存しない。
- `docker compose version` が成功し、`~/.docker/cli-plugins/docker-compose` の `realpath` は Nix 由来を指す。
- `~/.docker/config.json` は symlink 化されていない。
- `~/.docker/config.json` の `credsStore` / `credHelpers` から抽出した helper 名は `docker-credential-<helper>` 実バイナリへ解決できる。
- `credsStore: "desktop"` は残っていない（検出時は失敗）。
- CLI provider 検証は daemon 起動を前提にしない。
- Colima runtime 検証は Colima を使う環境で実行する。
- `docker context inspect colima` は `unix://$HOME/.config/colima/default/docker.sock` を返す。
- Homebrew 由来の `homebrew.mxcl.colima` は無効で、Nix 側 launchd のみ有効。
- Homebrew 由来の Compose plugin（`/opt/homebrew` / `/usr/local`）は使わない。
- Compose plugin 実体と `docker` / `colima` 実体の詳細な `/nix/store` 判定は、将来 `scripts/verify-nix-migration.sh` に切り出す。
- Colima の VM disk / pid / log / socket が `/nix/store` 配下に入っていた場合は失敗させる。
- daemon 起動は project devShell の責務ではない。

### DB clients

```sh
command -v mysql && mysql --version
command -v psql && psql --version
command -v sqlite3 && sqlite3 --version
type -P mysql psql sqlite3
type -P mysql psql sqlite3 | awk '/^(\\/nix\\/store\\/|\\/etc\\/profiles\\/per-user\\/|\\/nix\\/var\\/nix\\/profiles\\/)/ {ok++} END {exit !(ok==3)}'
! type -P mysql psql sqlite3 | rg -n '^(/opt/homebrew|/usr/local|/usr/bin)/'
```

期待:

- `mysql` は Nix 由来の `mysql-client` を指す。
- MySQL server はグローバル導入しない。必要時は devShell / service / container で別管理する。
- `psql` は Nix 由来の `postgresql_14` を指す。
- `sqlite3` は Nix 由来の `sqlite-interactive` を指す。
- `type -P mysql psql sqlite3` の解決先は `/nix/store` または Nix profile 配下のみを許容する。
- `/opt/homebrew`、`/usr/local`、`/usr/bin` 解決は通過条件にしない（検出時は失敗）。

### MAS

```sh
command -v mas
realpath "$(command -v mas)"
mas version
mas list
nix eval .#darwinConfigurations.ya.config.homebrew.masApps --json | jq -r 'to_entries[] | .value | if type == "object" and has("id") then .id else . end' | sort -u
comm -3 <(mas list | awk '{print $1}' | sort -u) <(nix eval .#darwinConfigurations.ya.config.homebrew.masApps --json | jq -r 'to_entries[] | .value | if type == "object" and has("id") then .id else . end' | sort -u)
```

期待:

- `mas` は実行可能である。
- `mas` の供給元は Nix package `mas` である。
- `mas list` と `homebrew.masApps` 宣言候補 ID の差分がない。
- 差分が残る場合は ID 収集が未完了なので、`homebrew.masApps` は宣言しない。

差分レビュー後の手動 remediation（routine 検証コマンドに含めない）:

- `homebrew.masApps` から削除した app は自動で消えない場合があるため、不要 ID が確定した場合にだけ `mas uninstall <APP_ID>` を手動実行する。
- `mas uninstall` は destructive 操作であり、install / update / uninstall の各操作は `mas` の version によって `sudo` / root 権限が必要になる場合がある。

### Homebrew

```sh
brew list --formula | sort
brew list --cask | sort
! brew list --formula | rg -n '^(docker-compose|docker-credential-helper|marksman|mysql|mysql-client|nodebrew|pyenv|pipx|python@3\\.9|python@3\\.10|python@3\\.11|rbenv-gemset)$'
! brew list --cask | rg -n '^(android-platform-tools|chromedriver|temurin|gcloud-cli|google-cloud-sdk|hashicorp-vagrant|font-noto-color-emoji|font-noto-emoji|font-zed-mono-nerd-font)$'
```

期待:

- Nix に移した formula は Homebrew から消えている。
- cask は nix-darwin の宣言と一致する。
- `android-platform-tools`、`chromedriver`、`temurin`、`gcloud-cli`、`google-cloud-sdk`、`hashicorp-vagrant` は cask 起点 CLI / SDK として残っていない。
- `docker-compose`、`docker-credential-helper`、`marksman`、`mysql`、`mysql-client`、`nodebrew`、`pyenv`、`pipx`、`python@3.9`、`python@3.10`、`python@3.11`、`rbenv-gemset` は formula 側に残っていない。
- MySQL server はグローバル導入対象外で、必要時のみ devShell / service / container で別管理する。
- `font-noto-color-emoji`、`font-noto-emoji`、`font-zed-mono-nerd-font` は cask 側に残っていない（Nix fonts へ移行済み）。
- 移行対象（formula/cask）に残存がないことは `brew list --formula` / `brew list --cask` の出力で確認する。
- 廃止対象 formula が残っていない。
- 複雑な差分検証が必要になったら `scripts/verify-nix-migration.sh` に切り出す。

### Homebrew と Nix の重複確認

Nix に移したコマンドが Homebrew にも残っていないか確認する。

```sh
zsh -i -c 'for c in git gh jq rg fzf node python ruby go php composer nvim docker colima; do printf "%s\t%s\n" "$c" "$(command -v "$c")"; done'
brew list --formula | sort
```

期待:

- 管理対象コマンドの解決元は Nix。
- 同じ CLI が Homebrew formula と Nix の両方に残っている項目は、残す理由を記録する。

### 手動導入ツール（任意棚卸し）

```sh
find ~/.cargo/bin ~/.bun/bin ~/.local/bin ~/bin ~/.nodebrew ~/.pyenv ~/.rbenv -maxdepth 2 -type f 2>/dev/null | sort || true
zsh -i -c 'command -v bun cargo-audit cargo-deny cargo-make diesel phpactor 2>/dev/null || true'
```

期待:

- 任意棚卸しなので、未導入コマンドは空表示でよい。
- Nix で置換済みのものは不要になっている。
- 残すものは例外として文書化されている。
- 削除前に、各コマンドの Nix 置換が動作している。

### devShell

```sh
nix develop ./templates/node -c node --version
nix develop ./templates/python -c python --version
nix develop ./templates/rust -c rustc --version
nix develop ./templates/ruby -c ruby --version
nix develop ./templates/go -c go version
nix develop ./templates/php -c php -v
nix develop ./templates/ocaml -c ocaml -version
```

期待:

- グローバル環境は維持される。
- プロジェクトでは devShell が優先される。
- devShell 内で `command -v` が devShell の toolchain を指す。

### 設定ファイル管理

Home Manager が既存 symlink と衝突しないか確認する。

```sh
ls -l ~/.zshrc ~/.gitconfig ~/.config/zsh ~/.config/nvim
home-manager build --flake .#ya
```

期待:

- `init.sh` が作った symlink を Home Manager 管理に移す手順が明確。
- 既存ファイルを消す前に Home Manager の配置先が確認できている。

### rollback

運用モードごとに rollback 境界を分ける。

- standalone Home Manager: Home Manager 世代だけを戻す。
- nix-darwin 統合: nix-darwin system 世代を戻す（Home Manager も同時に戻る）。

```sh
home-manager generations
home-manager switch --rollback
zsh -i -c 'echo ok'
```

nix-darwin 使用時:

```sh
sudo darwin-rebuild --list-generations
sudo darwin-rebuild switch --rollback
zsh -i -c 'echo ok'
```

特定世代へ戻す場合（nix-darwin system profile を直接使う）:

```sh
sudo nix-env -p /nix/var/nix/profiles/system --list-generations
sudo nix-env -p /nix/var/nix/profiles/system --switch-generation <GENERATION_NUMBER>
sudo /nix/var/nix/profiles/system/activate
zsh -i -c 'echo ok'
```

`<GENERATION_NUMBER>` はプレースホルダ。実行時は `sudo darwin-rebuild --list-generations`（または `sudo nix-env -p /nix/var/nix/profiles/system --list-generations`）で確認した実在の世代番号に置き換える。

期待:

- 直前世代へ戻せる。
- nix-darwin は `switch --rollback` または指定世代 `--switch-generation` + `system profile activate` で実際に戻せる。
- rollback 後も zsh が起動する。

## 移行順序

1. curl bootstrap の公開 endpoint、環境変数、鍵投入、`DOTFILES_RUN_SWITCH=0` の dry-run 手順を確定する。
2. `flake.nix` と `home.nix` の最小構成を作る。
3. Home Manager の運用モード（standalone か nix-darwin 統合）を確定し、適用コマンドを一本化する。
4. 多ユーザーへ広げる場合の `darwinConfigurations.<host>` / `homeConfigurations."<user>@<host>"` 分割を先に決める。
5. `init.sh` の symlink 管理を Home Manager に移す。
6. `.zshrc`、`config/zsh`、`config/nvim`、`.gitconfig` を Home Manager で配置する。
7. sops-nix、`.sops.yaml`、`secrets/common.yaml`、`secrets/hosts/<host>.yaml`、`secrets/users/<user>.yaml` を追加し、private key を repository 外で管理する。
8. `programs.*`（`git`、`gh`、`neovim`、`direnv`、`zsh`、`fzf`、`atuin`、`zoxide`）を先に定義し、`home.packages` との重複を除去する。
9. 共通 CLI とグローバル言語環境を Nix に追加する。
10. zsh plugin を Antidote の起動時 clone から Home Manager plugin 定義（src/file/order 固定）へ移す。
11. fzf は Homebrew key-bindings path source を削除し、`programs.fzf.enable = true; enableZshIntegration = false;` を設定する。Nix 由来 `key-bindings.zsh` のみを手動 source し、`completion.zsh` と `fzf --zsh` は使わない。`fzf-tab` は `compinit` 後に読み込み、`^I` は `expand-or-complete`、`^X^I` は `fzf-tab-complete` に固定する。
12. zsh テスト（`scripts/test-zsh-shortcuts.sh`、`scripts/test-zsh-key-operations-full.sh`）を Nix/HM 前提へ更新し、`plugins.txt` 非依存化と Homebrew 前提除去を行う。
13. Neovim は LSP を editor 管理（Mason 利用可）のまま維持し、`mason-null-ls` installer を無効化して Nix 供給へ統一する。
14. Mason 設定を `PATH = "skip"` に固定し、Mason formatter/linter bin（`stylua`/`markdownlint`）を purge する。
15. Neovim の TypeScript は `config/nvim/lua/omy/configs/mason-lspconfig.lua` の `tsserver` を `ts_ls` へ移行する。`lspconfig.lua` への `ts_ls` 追加は custom settings が必要な場合のみ行う。
16. Docker Compose v2 plugin discovery を `~/.docker/cli-plugins/docker-compose` symlink で固定し、`~/.docker/config.json` は mutable のまま維持する。
17. `pkgs.google-cloud-sdk.withExtraComponents (with pkgs.google-cloud-sdk.components; [ gke-gcloud-auth-plugin ])` を適用し、`gcloud components install` 運用を廃止する。
18. devShell templates を追加する。
19. 最小の nix-darwin 構成を先に導入し、`darwin-rebuild build/switch` が通る基盤を作る（Homebrew module 連携の土台化）。
20. nix-homebrew を導入し、Homebrew 本体と tap を所有させる（`autoMigrate = true`、必要時 `mutableTaps = false`）。
21. nix-darwin `homebrew.*` で cask / MAS を宣言し、同時に nix-darwin 側で fonts / launchd / defaults を宣言管理する。
22. Homebrew formula を Nix へ置換する。
23. `nodebrew`、`pyenv`、`pipx`、`rbenv-gemset` を廃止する。
24. `$HOME` 配下の手動導入ツールを Nix へ置換する。
25. PATH から旧手動導入ディレクトリを外す。
26. 検証コマンドをすべて実行する。
27. 置換済み Homebrew formula と手動導入物を削除する。
28. 例外リスト、MAS 手動アンインストール手順、日常運用手順を README に書く。

## レビュー対応表（今回修正分）

| 指摘ID | 対応内容 | 該当節 |
| --- | --- | --- |
| 1. `home.packages` の過剰所有 | `programs.*` 優先ルール（`git`、`gh`、`neovim`、`direnv+nix-direnv`、`zsh`、`fzf`、`atuin`、`zoxide`）を追加。`home.packages` は単機能 binary のみに限定し、重複禁止を明記 | `## レイヤー設計` / `## グローバル CLI` / `### Homebrew 移行先トレーサビリティ` |
| 2. HM standalone/統合の曖昧さ | standalone と nix-darwin 統合の責務、適用順序、rollback 境界を明確化。統合時 `home-manager.useGlobalPkgs/useUserPackages` を明記 | `## レイヤー設計` / `### rollback` |
| 3. Compose v2 plugin 発見性 | `docker-compose` 単体導入でなく plugin discovery 必須に修正。推奨実装を `~/.docker/cli-plugins/docker-compose` symlink に固定し、`~/.docker/config.json` 非symlinkを明記 | `#### Docker CLI` / `### Container / Docker / Colima` |
| 4. fzf Homebrew path 依存 | `config/zsh/completion.zsh` の Homebrew key-bindings path 依存を除去し、`programs.fzf.enable = true; enableZshIntegration = false;` + Nix 由来 key-bindings 手動 source（`completion.zsh`/`fzf --zsh` 不使用）・`^I` は classic completion、`^X^I` は `fzf-tab` を明記 | `## zsh 移行` / `### zsh` |
| 5. Neovim formatter 所有権競合 | LSP は Mason 許容、formatter/linter は Nix source of truth に固定。`mason-null-ls` installer から `stylua` 等を外す方針と判定コマンドを追加 | `## Neovim 移行` / `### Neovim` |
| 6. `tsserver -> ts_ls` の具体性不足 | 対象ファイルを明示し、移行タスクを順序へ追加 | `## Neovim 移行` / `## 移行順序` |
| 7. `kubectl` 所有権不足 | `kubectl` を Home Manager グローバル供給へ追加し、Kubernetes 検証コマンドへ `command -v kubectl` を追加 | `#### Kubernetes` / `## グローバル CLI` |
| 8. gcloud component ポリシー欠落 | `pkgs.google-cloud-sdk.withExtraComponents (with pkgs.google-cloud-sdk.components; [ gke-gcloud-auth-plugin ])` を明記し、`gcloud components install` 禁止を追加 | `#### Google Cloud SDK` / `### Homebrew 移行先トレーサビリティ` |
| 9. zsh/Neovim runtime state 棚卸し不足 | zsh history/compdump/cache と Neovim Jetpack/Mason/state/cache を inventory 化。Jetpack Phase 1 のネット依存と Phase 2 の Nix 化を明記 | `### zsh / Neovim の runtime/generated state 棚卸し` / `## Neovim 移行` |
| 10. zsh plugin 管理の具体性不足 | Home Manager plugin の `src/file/order` を具体化し、`p10k` の source path と読み込み順を定義 | `## zsh 移行` |
| 11. Git 設定移行の不足 | credential helper だけでなく `.gitconfig` 全設定（`init.defaultBranch`、`alias.graph` など）移行を明記 | `## Git 移行` |
| 12. MAS cleanup 制約 | `mas uninstall <APP_ID>` を routine 検証コマンドから分離し、差分レビュー後の手動 remediation（destructive・権限注意）として明記。加えて install/update/uninstall 全操作で権限昇格の可能性を追記 | `## cask / GUI アプリ` / `### MAS` |
| 13. GUI 配置境界の曖昧さ | Home Manager は stateVersion にかかわらず `~/Applications/Home Manager Apps` 配下（`<25.11` symlink、`>=25.11` copyApps default copy）、nix-darwin は `/Applications/Nix Apps` と明記 | `## 目標構成` / `## cask / GUI アプリ` |
| 14. zsh テストの Antidote/Homebrew 前提 | `scripts/test-zsh-shortcuts.sh` と `scripts/test-zsh-key-operations-full.sh` の移行タスクと期待挙動（`plugins.txt` 非依存、Homebrew 前提除去）を追加 | `## zsh 移行` / `### zsh` |
| 15. Git helper リセット順序 | `credential.*.helper = [ "" "!gh auth git-credential" ]` を GitHub/Gist 両方に明記し、`helper =` -> `helper = !...` の順序保持を追記 | `## Git 移行` |
| 16. Bun PATH 記述の誤り | `.zshrc` の実態（Rancher Desktop PATHのみ）へ修正し、Bun は PATH 汚染 inventory 側で扱うよう修正 | `### Node / JavaScript / TypeScript / Bun` / `## zsh 移行` / `### PATH 汚染確認` |
| 17. `tsserver -> ts_ls` の repo 依存性 | `mason-lspconfig.lua` のみ変更対象、`lspconfig.lua` は custom settings 時のみ `ts_ls` override 追加と明記 | `## Neovim 移行` / `## 移行順序` |
| 18. Mason PATH 先頭挿入問題 | `PATH = "skip"` を必須化し、`stylua`/`markdownlint` の Mason bin/package purge 手順を追加 | `## Neovim 移行` / `### Neovim` |
| 19. MAS jq 抽出式の不整合 | `to_entries[] | .value | if type == "object" and has("id") then .id else . end` に統一 | `## cask / GUI アプリ` / `### MAS` |
| 20. Homebrew 所有権の層分離不足 | 導入順を `最小 nix-darwin -> nix-homebrew（Homebrew本体/tap）-> nix-darwin homebrew.* + fonts/defaults/launchd` に固定 | `## レイヤー設計` / `## 移行順序` |
| 21. zsh-completions 検証の曖昧さ | `home.packages` に `zsh-completions` を追加し、検証を「Home Manager package closure/profile に `zsh-completions` が存在すること」+「既知 completion file が `$fpath`/`compinit` 経由で解決できること」へ強化 | `## グローバル CLI` / `## zsh 移行` / `### zsh` |
| 22. 失敗条件コマンドの真偽誤り | `&& false || true` を廃止し、`! ... | rg ...` / `if ...; then false; fi` で match 時に確実に失敗する形へ修正 | `## Neovim 移行` / `## zsh 移行` / `### Container / Docker / Colima` / `### DB clients` |

## 完了条件

- `home-manager build --flake .#ya` が通る。
- `home-manager switch --flake .#ya` が通る。
- Node、Python、Rust、Ruby、Go、PHP、OCaml がグローバルで使える。
- グローバル言語環境は Nix 由来。
- project devShell で言語 version を上書きできる。
- Neovim が起動し、LSP は editor 管理（Mason 利用可）で動作し、formatter / linter は通常時 Nix、devShell 内では active devShell から解決する。
- zsh がエラーなく起動する。
- zsh plugin が起動時 clone に依存しない。
- `.gitconfig` が `/opt/homebrew/bin/gh` を直接参照しない。
- Nix へ移した Homebrew formula が Homebrew に残っていない。
- cask は nix-darwin の宣言と一致する。
- `~/.cargo/bin`、`~/.bun/bin`、`~/.nodebrew`、`~/.pyenv`、`~/.rbenv`、`pipx` に依存しない。
- 残す手動導入ツールは例外として明記されている。
- rollback 手順が確認済み。

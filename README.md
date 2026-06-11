# dotfiles

Nix flake として再利用できる `nix-darwin` + `home-manager` 管理 dotfiles。

ユーザーごとのローカル flake を `~/.config/dotfiles/flake.nix` に生成し、その flake から Home Manager と nix-darwin を適用します。

## 初回導入

macOS の新規環境では `scripts/bootstrap.sh` を使います。

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/scripts/bootstrap.sh | bash
```

特定の commit / tag に固定する場合:

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/<tag-or-commit>/scripts/bootstrap.sh | DOTFILES_BOOTSTRAP_SOURCE_REF=<tag-or-commit> bash
```

bootstrap は必要に応じて Nix を用意し、ローカル flake の生成と適用まで実行します。

既存の Home Manager 管理対象ファイルは `*.before-home-manager` に退避してから置き換えます。

sudo の Touch ID / Apple Watch 認証は nix-darwin 適用後に有効になります。初回 bootstrap で Nix や nix-darwin を入れる前の sudo 認証は、通常のパスワード入力が必要になる場合があります。

## 更新と適用

導入済みの環境では、通常 `dotfiles update` で最新版を取り込んでから設定を適用します。
対象を省略すると `all` として扱い、Home Manager の後に nix-darwin を適用します。

```sh
dotfiles update
dotfiles update home
dotfiles update darwin
dotfiles update all
```

更新せずに、現在のローカル flake のまま再適用する場合は `switch` を使います。

```sh
dotfiles switch
dotfiles switch home
dotfiles switch darwin
dotfiles switch all
```

適用時に呼ばれるコマンド:

```sh
home-manager switch --flake ~/.config/dotfiles#<user>
sudo darwin-rebuild switch --flake ~/.config/dotfiles#<host>
```

## ローカル flake の生成

bootstrap を使わずにローカル flake だけを作る場合は `dotfiles init` を使います。

```sh
nix run github:wthrk/dotfiles -- init
nix run github:wthrk/dotfiles -- init --user alice --host macbook --system aarch64-darwin
nix run github:wthrk/dotfiles -- init --source github:wthrk/dotfiles --force
```

## 秘密情報復旧

新規マシン復旧用の bootstrap secret は YubiKey PIV 領域に保存します。保存対象は `bitwarden-client-id` と `bitwarden-client-secret` だけです。

```sh
dotfiles secrets yubikey enroll-primary
dotfiles secrets yubikey enroll-spare
dotfiles secrets verify-yubikey
```

`enroll-primary` と `enroll-spare` は、接続中の YubiKey が 1 本だけであることを前提に、CLI secret input port から `bitwarden-client-id` と `bitwarden-client-secret` を受け取って保存します。secret 本文は CLI 引数では受け取りません。`setup`、`put`、`get` は低水準コマンドで、`get` は stdout が terminal の場合に平文出力を拒否します。

復旧対象の `gpg-secret-key-backup` と `password-store-remote` はユーザー個人の Bitwarden vault に保存します。dotfiles は vault へのアクセスを CLI ではなく SDK/API adapter 境界で扱い、shell script は credential、URL、fingerprint、API key を argv/stdin/env で中継しません。

```sh
dotfiles secrets restore-gpg
dotfiles gpg export-ssh-public-key
dotfiles secrets restore-pass
```

`restore-gpg` は個人 vault から encrypted envelope を取得し、接続中 YubiKey で復号して GPG 鍵リングへ復元します。import 後に authentication subkey の keygrip を gpg-agent の SSH key list へ登録し、gpg-agent SSH support が利用可能であることを確認します。

`restore-pass` は個人 vault から `password-store-remote` を取得し、`~/.password-store` が存在しないことを確認してから、GPG authentication subkey 経由の SSH agent 認証で clone します。clone 後に store が `pass` から読めることを確認します。

backup envelope の照合と `password-store-remote` の登録は provisioning 経路で行います。

```sh
dotfiles secrets gpg-backup register
dotfiles secrets pass-remote register
```

`gpg-backup register` は既存の `gpg-secret-key-backup` encrypted envelope が解決済み primary fingerprint、接続中 YubiKey recipient、primary/spare の 2 recipient 以上条件を満たすか確認します。`pass-remote register` は configured origin を優先し、origin が無い場合だけ CLI/app 側の controlling TTY input port で `password-store-remote` を受けます。URL はログ、エラー本文、診断出力に含めません。

vault への到達確認は `dotfiles secrets verify-yubikey --check vault` または `--all` で行います。

gpg-agent の SSH support 設定（`gpg-agent.conf` の `enable-ssh-support` と `pinentry-program`）は Home Manager 管理です。`config/zsh/env.zsh` は `GPG_TTY` を設定し、`${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在する場合だけ `SSH_AUTH_SOCK` を上書きします。

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

## 開発環境

このリポジトリを編集する場合は、初回だけ direnv を許可します。

```sh
direnv allow .
```

このディレクトリでは `direnv` が flake の devShell を読み込みます。検証や内部開発タスクは `cargo xtask` で実行します。

```sh
cargo xtask check
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

## 検証

```sh
cargo xtask check
```

通常の静的検証を実行します。Rust、Nix、shell script、GitHub Actions workflow を確認します。

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

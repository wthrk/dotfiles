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

新規マシン復旧用の bootstrap secret は YubiKey PIV 領域に保存します。通常は primary 登録と spare 登録だけを使います。

```sh
dotfiles secrets yubikey enroll-primary
dotfiles secrets yubikey enroll-spare
dotfiles secrets verify-yubikey
dotfiles secrets yubikey rotate-bws-token
```

`enroll-spare` は primary YubiKey から `bw-email`、`bw-password`、`bws-access-token` を読み出した直後に spare YubiKey を選択します。1 本ずつしか接続できない場合は、この時点で primary を抜いて spare を挿し、表示された prompt で Enter を押します。非対話実行では `--primary-serial` と `--spare-serial` を指定します。

`rotate-bws-token` は対話実行では新しい token を一度だけ読み取り、利用者が選択した YubiKey を更新します。primary とすべての spare を更新対象にし、summary に出た serial を見て対象全本が更新済みであることを確認してください。非対話実行では `--serial` と `--stdin` を指定して 1 本ずつ更新します。

`setup`、`put`、`get` は低水準コマンドです。直接使う場合でも secret 本文は CLI 引数では受け取らず、prompt または stdin から読みます。`get` は stdout が terminal の場合は平文出力を拒否するため、pipe または redirect 先を明示します。

### GPG 鍵リング復元と SSH 公開鍵

新規マシンでは `restore-gpg` で `gpg-secret-key-backup` encrypted envelope を取得し、接続中 YubiKey で復号して GPG 鍵リングへ復元します。import 後に authentication subkey の keygrip を gpg-agent の SSH key list へ登録し、gpg-agent SSH support が利用可能であることを確認します。

```sh
dotfiles secrets restore-gpg
dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>
```

`export-ssh-public-key` は GPG authentication subkey 由来の OpenSSH 公開鍵 1 行を stdout に出力します。秘密鍵素材は出力せず、GitHub SSH keys 登録用途に使います。

### password-store 復元

GPG 鍵リング復元と SSH 公開鍵の GitHub 登録が済んだら、`restore-pass` で private `password-store` repository を復元します。Bitwarden Secrets Manager から `password-store-remote`（`git@github.com:<owner>/<repo>.git` 形式）を取得し、`~/.password-store` が存在しないことを確認してから、GPG authentication subkey 経由の SSH agent 認証で clone します。clone 後に store が `pass` から読めること（`.gpg-id` が存在し、store entry を復元済み秘密鍵で実際に復号できること）を確認します。`.gpg-id` の複数 recipient（共有・予備鍵）や email・user-id 形式の recipient も対応し、entry が無い空 store ではいずれか 1 つの recipient の秘密鍵を保持していれば読めるとみなします。

```sh
dotfiles secrets restore-pass
```

clone は `git2` と SSH agent だけを使い、`git` CLI と GitHub API は使いません。gpg-agent SSH support の利用可否（SSH agent socket の解決と authentication subkey の識別）は `restore-gpg` が確認・gate するため、`restore-pass` はその setup を信頼して clone します。`~/.password-store` が既に存在する場合、remote URL が GitHub SSH clone URL でない場合、接続先 `github.com` の SSH host key が GitHub 公表の host key と一致しない場合、clone 後 store を `pass` が読めない場合（store entry を復号できない、または空 store でいずれの recipient の秘密鍵も持たない場合）は停止します。

backup envelope の登録・recipient 追加と `password-store-remote` の登録は provisioning 経路で行います。

```sh
dotfiles secrets gpg-backup register --primary-fingerprint <40-hex-fingerprint> [--serial <serial>]
dotfiles secrets gpg-backup add-spare [--unwrap-serial <serial>] --spare-serial <serial> [--yes]
dotfiles secrets pass-remote register [--url git@github.com:<owner>/<repo>.git] [--yes]
```

これらの provisioning コマンドは Bitwarden Secrets Manager への書込み（project / secret の create・update）に、書込み可能な provisioning 用 access token を使います。この token は YubiKey に保存する read-only `dotfiles-secret-recovery-reader` token ではなく（reader token は読み取り専用で書込みに使えず、provisioning は YubiKey へ reader token を保存する前の管理 plane 操作です）、hidden prompt（TTY）または pipe（stdin）から保護 buffer へ受け取ります。token を CLI 引数（argv）・ログ・shell history・永続環境変数・永続一時ファイルへは載せません。この token は初期登録後に失効させてください。

`register` は既存環境の GPG secret key を encrypted envelope 化し、接続中 YubiKey の recipient を 1 件作って Bitwarden Secrets Manager へ登録します。`add-spare` は既存 envelope を復号して同一 DEK を spare YubiKey の recipient へ追加し、stale overwrite 防止の更新識別子が一致する場合だけ更新します。`gpg-backup register`/`add-spare` は BWS 書込みに provisioning 用 access token を使いますが、recipient wrap（PIV slot `82` 公開鍵での DEK wrap）と既存 recipient による DEK unwrap には引き続き接続中 YubiKey を使うため、recipient 用の `--serial`/`--unwrap-serial`/`--spare-serial` を指定します。`pass-remote register` は private `password-store` repository の clone URL（`git@github.com:<owner>/<repo>.git` 形式）を Bitwarden Secrets Manager の復旧 project へ create または update する保管コマンドで、`gpg-backup register`/`add-spare` と対称な provisioning 経路です。`pass-remote register` は YubiKey を一切使わないため `--serial` を持ちません。clone URL は認証・復号・署名・外部アクセス能力を与える credential ではないため、provisioning 入力では非秘匿として扱い、`--url <value>` 引数・可視プロンプト（対話実行で入力をエコー）・pipe（stdin）のいずれの方式でも入力できます（`--url` を指定すればその値を使い、未指定なら terminal で可視プロンプト・非 terminal で pipe から読みます）。ただし private repository の所在を示す値のため、ログ・エラー本文・診断出力には含めません。既存値の上書きは stale overwrite 防止の更新識別子が一致する場合だけ行います。これらの provisioning コマンドを非対話実行で上書き更新する場合は `--yes` を指定します。

gpg-agent の SSH support 設定（`gpg-agent.conf` の `enable-ssh-support` と `pinentry-program`）は Home Manager 管理です。`config/zsh/env.zsh` は `GPG_TTY` を設定し、`${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在する場合だけ `SSH_AUTH_SOCK` を上書きします。

### Bitwarden Password Manager login

`bw-login` は Bitwarden Password Manager の CLI に login / unlock します。接続中の YubiKey から `bw-email` と `bw-password` を取得し、YubiKey OTP を入力させたうえで、`bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>`（2FA method 3 / YubiKey OTP）と `bw unlock --passwordenv BW_PASSWORD --raw` を `bw` CLI で実行します。`bw` CLI はこの login / unlock だけに許可された唯一の外部 CLI 例外で、secret 取得や永続保存用途には使いません。

```sh
dotfiles secrets bw-login [--serial <serial>] [--email <email>]
```

master password は子プロセスの `BW_PASSWORD` 環境変数でだけ渡し、CLI 引数（argv）・ログ・永続保存には載せません。`BW_PASSWORD` は保存しません。login email は通常 YubiKey 内の `bw-email` を使い、override が必要な場合だけ `--email <email>` を指定します。複数本の YubiKey が接続されている場合は `--serial <serial>` で対象を固定します。

`bw unlock --raw` が出力する `BW_SESSION` 値はコマンドが利用者へ surface するだけで、disk や dotfiles へ永続化しません。表示された値を使って `export BW_SESSION=...` を実行すれば、以降の `bw` 操作の session として使えます。

`bw-login` は Bitwarden account 側の 2FA / passkey 登録を自動化しません。primary と spare の両方の YubiKey をあらかじめ Bitwarden account の 2FA（Yubico OTP）として登録しておく必要があります（spare YubiKey の Bitwarden 側登録は `bw-login` では行いません）。

#### primary / spare の manual login validation

primary と spare のどちらの YubiKey でも login / unlock が完了することを、次の手順で確認します。`bw-login` は status 分岐をせず無条件で `bw login` → `bw unlock` を実行するため、各 manual validation は `bw` CLI が未ログイン状態（unauthenticated）であることを前提とします。直前の validation などで既に login 済みの場合は、`bw` CLI が「already logged in」で失敗するのを避けるため、次の YubiKey を検証する前に operator が `bw logout` を実行してから再実行してください。これは operator が手動で行う検証手順上の前提を示す注記であり、tool 側の `bw` CLI 用途を login / unlock 以外へ広げるものではありません。

1. primary YubiKey を接続し、`dotfiles secrets bw-login`（または `dotfiles secrets verify-yubikey --check bw-login`）を実行して、OTP 入力後に login / unlock が成功することを確認します。
2. primary を抜いて spare YubiKey を接続します。直前の手順で `bw` CLI が login 済みのままの場合は、ここで operator が `bw logout` を実行して未ログイン状態に戻してから、同じコマンドを実行します。複数本を同時接続する場合は `--serial <serial>` で spare を対象に指定し、spare でも login / unlock が成功することを確認します。
3. `dotfiles secrets verify-yubikey --check bw-login` と `dotfiles secrets verify-yubikey --all` は、`bw-login` と同じ Bitwarden Password Manager の login / unlock 到達確認を実行します（session key は確認専用のため surface せず破棄します）。引数なしの `dotfiles secrets verify-yubikey` は ローカル保管 確認だけを行い、外部の bw-login 項目は機械可読状態値 `skipped` のまま残します。

利用できるのは `--serial` と `--email` だけです。`--check bws` / `--check bw-login` / `--all` は外部確認を要求する option で、到達できない場合は明示的に失敗します。

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

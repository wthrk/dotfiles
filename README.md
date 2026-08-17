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

このコマンドは全ユーザー共通です。何が入るかはマシンの状態で決まり、指定は要りません。まだ誰も
nix-darwin を適用していないマシンと、既に自分が適用しているマシンでは、home 層と system 層の両方が
入ります。既に別のユーザーが nix-darwin を適用しているマシンでは、自分の home 層だけが入り、sudo は
訊かれません。

既存の Home Manager 管理対象ファイルは `*.before-home-manager` に退避してから置き換えます。

sudo の Touch ID / Apple Watch 認証は nix-darwin 適用後に有効になります。初回 bootstrap で Nix や nix-darwin を入れる前の sudo 認証は、通常のパスワード入力が必要になる場合があります。

## キーリマップ（Karabiner-Elements）

CapsLock→Ctrl の割り当ては `config/karabiner/karabiner.json` にあり、Karabiner-Elements 本体は Homebrew
cask で入ります。[上流の導入手順](https://karabiner-elements.pqrs.org/docs/getting-started/installation/)は
次の 4 つの許可を要求します。いずれも macOS の承認が要るため宣言できず、初回適用後に手で与えます。以下は
macOS 26 のシステム設定のペイン名です。

- 一般 → ログイン項目と機能拡張 → アプリのバックグラウンドでのアクティビティ で、Karabiner の特権/非特権の
  バックグラウンドサービスを許可します。
- プライバシーとセキュリティ → アクセシビリティ で Karabiner-Elements を許可します。macOS の説明どおり
  「コンピュータの制御」を許可するもので、この 4 つで最も広い権限です。
- プライバシーとセキュリティ → 入力監視 で Karabiner-Elements を許可します（上流によればアクセシビリティを
  許可すると自動で付きます）。
- 一般 → ログイン項目と機能拡張 → 機能拡張 で、仮想キーボード/マウスの「ドライバ機能拡張」を許可します。

`karabiner.json` はリポジトリ側が正本です。Karabiner の GUI から設定を変えると symlink が実ファイルに
置き換わりますが、次の適用で宣言側に戻ります。変更はリポジトリに入れてください。

## 入力ソースの強制（Hammerspoon）

tty アプリ（Ghostty、iTerm2、Terminal.app）と Zed をアクティブにした瞬間に入力ソースを ABC へ戻す処理は
`config/hammerspoon/init.lua` にあり、`~/.hammerspoon/init.lua` へリンクします。Hammerspoon 本体は Homebrew
cask で入ります。`init.lua` はリポジトリ側が正本です。

起動は `nix/modules/hammerspoon.nix` が宣言する LaunchAgent が担います。ログイン時と、cask が
`/Applications/Hammerspoon.app` を置いた時点の 2 つが起動条件です。適用は home 層（LaunchAgent）から
system 層（cask）の順に走るため、初回適用では後者が起動を受け持ちます。

`init.lua` を変えた適用のあとは、Hammerspoon の Reload Config で読み直します。Hammerspoon は設定ファイルの
変更を自分では読み直しません。

## 更新と適用

導入済みの環境では、通常 `dotfiles update` で最新版を取り込んでから設定を適用します。`update` はローカル
flake の `dotfiles` input だけを更新（`nix flake update dotfiles --flake <config_dir>`）し、repo の committed
lock にある全 input の pin へ追随してから、既存の `switch` と同じ適用処理を行います。非 macOS では
`dotfiles update home` を使ってください。

```sh
dotfiles update
```

ローカルに適用状態ファイルや marker は持たず、冪等性は nix 自身の lock / profile と switch の冪等性に委ねます。
適用された更新の概要（version 差分と change_items）は repo の `docs/update-history/*.toml`（nightly CI が記録）
にあり、`dotfiles update-history show` でいつでも閲覧できます。

更新せずに、現在のローカル flake のまま再適用する場合は `switch` を使います。`update` / `switch` はいずれも
適用対象を取り、`home` / `darwin` で部分適用できます。

対象を省略したときに何が適用されるかは、導入時と同じくマシンの状態で決まります。system 層を自分が
持つマシンでは Home Manager に続いて nix-darwin を適用し、別のユーザーが持つマシンでは Home Manager
だけを適用します。後者で nix-darwin を含む対象（`darwin` と `all`）を明示した場合は、理由を表示して
止まります。

```sh
dotfiles switch
dotfiles switch home
dotfiles switch darwin
dotfiles switch all
```

自動更新はマシンに 1 つの daemon（`org.dotfiles.auto-update`）が担い、そのマシンでローカル flake を
持つ全ユーザーを更新します。各ユーザーの lock 更新と Home Manager はそのユーザー権限で実行し、
system 層は所有者の flake からだけ適用します。

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

これらの provisioning コマンドは Bitwarden Secrets Manager への書込み（secret の create・update）に、利用者が用意した個人用 BWS access token を使います。実行前に Bitwarden Secrets Manager 側で project `dotfiles-secret-recovery` を作成し、登録・更新用 token から同名 project が 1 件だけ見える状態にしてください。`dotfiles` / `scripts/provision-secret-recovery-source.sh` は project を作成せず、見つからない場合または複数見える場合は停止します。YubiKey の `bws-access-token` には、書込み用 token ではなく、復旧時に `dotfiles-secret-recovery` project の必要 secret を読める最小権限の復旧用 token を保存します。この token は `restore-gpg` / `restore-pass` / `verify-yubikey --check bws` の BWS 読取に使い、`verify-yubikey --check bws` で有効性を確認します。token は hidden prompt（TTY）または pipe（stdin）から保護 buffer へ受け取り、CLI 引数（argv）・ログ・shell history・永続環境変数・永続一時ファイルへは載せません。

`register` は既存環境の GPG secret key を encrypted envelope 化し、接続中 YubiKey の recipient を 1 件作って Bitwarden Secrets Manager へ登録します。`add-spare` は既存 envelope を復号して同一 DEK を spare YubiKey の recipient へ追加し、stale overwrite 防止の更新識別子が一致する場合だけ更新します。`gpg-backup register`/`add-spare` は BWS 書込みに登録・更新用 BWS access token を使いますが、recipient wrap（PIV slot `82` 公開鍵での DEK wrap）と既存 recipient による DEK unwrap には引き続き接続中 YubiKey を使うため、recipient 用の `--serial`/`--unwrap-serial`/`--spare-serial` を指定します。`pass-remote register` は private `password-store` repository の clone URL（`git@github.com:<owner>/<repo>.git` 形式）を Bitwarden Secrets Manager の復旧 project へ create または update する保管コマンドで、`gpg-backup register`/`add-spare` と対称な provisioning 経路です。`pass-remote register` は YubiKey を一切使わないため `--serial` を持ちません。clone URL は認証・復号・署名・外部アクセス能力を与える credential ではないため、provisioning 入力では非秘匿として扱い、`--url <value>` 引数・可視プロンプト（対話実行で入力をエコー）・pipe（stdin）のいずれの方式でも入力できます（`--url` を指定すればその値を使い、未指定なら terminal で可視プロンプト・非 terminal で pipe から読みます）。ただし private repository の所在を示す値のため、ログ・エラー本文・診断出力には含めません。既存値の上書きは stale overwrite 防止の更新識別子が一致する場合だけ行います。これらの provisioning コマンドを非対話実行で上書き更新する場合は `--yes` を指定します。

gpg-agent の SSH support 設定（`gpg-agent.conf` の `enable-ssh-support` と `pinentry-program`）は Home Manager 管理です。`config/zsh/env.zsh` は `GPG_TTY` を設定し、`${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在する場合だけ `SSH_AUTH_SOCK` を上書きします。

### Bitwarden Password Manager login

`bw-login` は Bitwarden Password Manager の CLI に login / unlock します。接続中の YubiKey から `bw-email` と `bw-password` を取得し、YubiKey OTP を入力させたうえで、`bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>`（2FA method 3 / YubiKey OTP）と `bw unlock --passwordenv BW_PASSWORD --raw` を `bw` CLI で実行します。`bw` CLI はこの login / unlock だけに許可された唯一の外部 CLI 例外で、secret 取得や永続保存用途には使いません。

```sh
dotfiles secrets bw-login [--serial <serial>] [--email <email>]
```

master password は子プロセスの `BW_PASSWORD` 環境変数でだけ渡し、CLI 引数（argv）・ログ・永続保存には載せません。`BW_PASSWORD` は保存しません。login email は通常 YubiKey 内の `bw-email` を使い、override が必要な場合だけ `--email <email>` を指定します。複数本の YubiKey が接続されている場合は `--serial <serial>` で対象を固定します。

`bw unlock --raw` が出力する `BW_SESSION` 値はコマンドが利用者へ surface するだけで、disk や dotfiles へ永続化しません。表示された値を使って `export BW_SESSION=...` を実行すれば、以降の `bw` 操作の session として使えます。

`bw-login` は Bitwarden account 側の 2FA / passkey 登録を自動化しません。primary と spare の両方の YubiKey をあらかじめ Bitwarden account の 2FA（Yubico OTP）として登録しておく必要があります（spare YubiKey の Bitwarden 側登録は `bw-login` では行いません）。

primary / spare 双方での login / unlock 検証手順は
[`docs/secret-recovery/initial-provisioning-runbook.md`](docs/secret-recovery/initial-provisioning-runbook.md)
を参照してください。

`bw-login` が受け付けるフラグは `--serial` と `--email` だけです。`verify-yubikey` は `--serial` に加えて外部確認を要求する option `--check bws` / `--check bw-login` / `--all`（到達できない場合は明示的に失敗します）を受け付け、`--email` は bw-login 確認の login email override にのみ適用されます。

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

静的検証と Tart VM 統合検証をまとめて実行する場合:

```sh
cargo xtask check all --source-hash "$(git rev-parse HEAD)"
```

個別に実行する場合:

```sh
cargo xtask check static
```

Tart VM を使う runtime 検証:

```sh
cargo xtask check runtime --source-hash "$(git rev-parse HEAD)"
```

ゲストは渡した commit を GitHub から checkout し、そこでゲスト実行器をビルドして起動します。push 済みの
commit を渡してください。`runtime-integration.yml` の runner も同じ commit を checkout し、ゲスト上で同じ
手順を踏みます。一致するのはここまでで、ゲスト OS の版は既定では揃いません。CI は macOS 26 の runner、
手元の既定イメージ `sequoia-runtime-base` は macOS 15 系です。検証対象が bootstrap と darwin switch である
以上、この版差は検証内容の差になります。

既定イメージは初回に自分で作ります。ゲストの bootstrap と darwin switch は公開イメージの容量に収まらないため、
仮想ディスクと APFS container を広げたイメージを packer で作ります。clone 元の公開イメージは
`packer/runtime-integration-base.pkr.hcl` の `vm_base_name` が指しており、build がレジストリから pull します。

```sh
packer init packer/runtime-integration-base.pkr.hcl
packer build packer/runtime-integration-base.pkr.hcl
```

CI と同じ macOS 26 で確認する場合は、同ファイルの `vm_base_name` を
`ghcr.io/cirruslabs/macos-tahoe-vanilla:latest` に、`vm_name` を別の名前に変えてから build し、その `vm_name`
を `DOTFILES_TART_IMAGE` に渡します。

zsh 設定の実挙動検証（補完、キーバインド、PATH、起動時出力）は devShell 内で bats を直接起動します。
Rust のビルド成果物を必要としないため `cargo xtask check` の下には置いていません。

bats は適用済みの構成を起動して観測します。作業ツリーの変更を見るには、作業ツリーを参照元にして適用してから
起動します。

```sh
dotfiles init --source "path:$PWD" --force
dotfiles switch home
bats tests/zsh
```

`--force` は既存の `~/.config/dotfiles/flake.nix` を置き換えるために要ります。検証後は
`--source github:wthrk/dotfiles --force` で参照元を戻します。戻さないと `dotfiles update` が作業ツリーを
追い続けます。

## 無人更新の運用

nightly の `flake.lock` 全 input bump と auto-merge ゲート、switch 時の Homebrew 無人 upgrade の前提は
[`docs/automation/README.md`](docs/automation/README.md) を参照してください。

- nightly bump の対象・auto-merge を止めるゲート: [`docs/automation/nightly-lock-bump.md`](docs/automation/nightly-lock-bump.md)
- 無人 cask upgrade の明示受容と成果物固定の強制機構: [`docs/automation/homebrew-cask-pinning.md`](docs/automation/homebrew-cask-pinning.md)

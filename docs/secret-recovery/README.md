# 新規マシン秘密情報復旧基盤

この文書は、新しい macOS マシンで `dotfiles` を導入したあと、開発に必要な秘密情報基盤を復旧するための設計を定義する。対象は GnuPG secret key、GPG authentication subkey による GitHub SSH identity、private `password-store` repository、`pass` の利用環境、Bitwarden Password Manager の CLI login である。

復旧の入口には YubiKey を使う。YubiKey には Bitwarden master password と Bitwarden Secrets Manager access token を保存する。Bitwarden Secrets Manager には GPG secret key backup と `password-store` の remote URL を保存する。GPG secret key を復元したあと、GPG authentication subkey を SSH identity として使い、GitHub から private `password-store` repository を SSH clone する。

## 目的

- 新規マシンで秘密情報基盤を再構築する手順を `dotfiles` CLI に集約する。
- 復旧に必要な bootstrap secret を YubiKey と Bitwarden Secrets Manager に分離して保持する。
- GitHub API や keyserver に依存せず、GPG authentication subkey 由来の SSH identity で private repository を取得する。
- 平文 secret を CLI 引数、ログ、一時ファイル、永続環境変数に残さない。
- 破壊的な YubiKey reset や既存 credential の削除を自動化しない。

## 復旧対象

- GnuPG secret key
- GPG encryption subkey による `pass` 復号環境
- GPG authentication subkey による GitHub SSH identity
- GPG signing subkey による Git signing 環境
- private `password-store` repository
- Bitwarden Password Manager の CLI login / unlock

## Secret の置き場所

| 場所 | 保存する secret | 用途 |
| --- | --- | --- |
| YubiKey | `bw-password` | Bitwarden Password Manager の CLI login / unlock |
| YubiKey | `bws-access-token` | Bitwarden Secrets Manager から復旧情報を取得 |
| Bitwarden Secrets Manager | `gpg-secret-key-backup` | GPG secret key の復元 |
| Bitwarden Secrets Manager | `password-store-remote` | private `password-store` repository の clone URL |
| Bitwarden Password Manager | Web service passwords、passkeys、TOTP、recovery codes | ユーザ向け password manager |
| `pass` / `~/.password-store` | Bitwarden CLI API `client_id` / `client_secret`、UNIX operational secrets | CLI やローカル運用向け secret |
| GitHub | GPG authentication subkey 由来の SSH public key | private repository clone |

## 責務分担

### YubiKey

YubiKey は復旧入口の bootstrap secret を保持する。対象は `bw-password` と `bws-access-token` の 2 種類だけである。

YubiKey 操作は Rust crate から行い、`ykman` CLI は使わない。PIV の reset や global state を破壊する操作は実装しない。書き込み対象はこの機能用に確保した領域だけに限定し、既存の FIDO2 / OTP / OpenPGP / PIV credential を reset しない。既存領域と衝突する場合は停止する。

詳細設計は [YubiKey 秘密情報保存設計](./yubikey-secret-storage-design.md) に置く。

### Bitwarden Secrets Manager

Bitwarden Secrets Manager は復旧に必要な機械向け secret を保持する。対象は `gpg-secret-key-backup` と `password-store-remote` である。

復旧本線では公式 `bitwarden` Rust SDK を使う。`bw` CLI は Bitwarden Secrets Manager からの取得には使わない。access token は YubiKey から取得し、必要な API 呼び出しの範囲だけで保持する。

### Bitwarden Password Manager

Bitwarden Password Manager は Web service passwords、passkeys、TOTP、recovery codes を保持する。CLI 操作は login / unlock だけを対象にし、`bw` CLI の用途は `dotfiles secrets bw-login` に限定する。

Bitwarden master password は YubiKey から取得し、`BW_PASSWORD` として子プロセスにだけ渡す。`BW_PASSWORD` は保存しない。`BW_SESSION` の扱いは `bw-login` の設計 PR で確定する。

### GnuPG / SSH

GPG key は software key として運用する。GPG key material は YubiKey に入れない。GPG secret key backup は Bitwarden Secrets Manager に保存する。

`pass` には encryption subkey を使う。GitHub SSH identity には authentication subkey を使う。Git signing には signing subkey を使う。GitHub private repository の取得は GPG authentication subkey による SSH clone で行い、GitHub API は使わない。keyserver は使わない。既存の `~/.ssh/id_ed25519` は新規運用では使わない。

GPG keyring 操作は `gpgme` を使う。OpenPGP public key 操作が必要な場合は `sequoia-openpgp` を使う。`gpg` CLI は通常実装では使わない。

### Git

private `password-store` repository の clone は `git2` と SSH agent を使う。`git` CLI は復旧本線では使わない。SSH agent には gpg-agent の SSH support を使い、GPG authentication subkey 由来の identity を GitHub に提示する。

## 復旧フロー

1. `dotfiles secrets yubikey setup` で YubiKey 5 PIV の利用前提と専用保存領域を確認する。
2. `dotfiles secrets yubikey put bw-password` で Bitwarden master password を YubiKey に保存する。
3. `dotfiles secrets yubikey put bws-access-token` で Bitwarden Secrets Manager access token を YubiKey に保存する。
4. `dotfiles secrets restore-gpg` で Bitwarden Secrets Manager から GPG secret key backup を取得し、GPG secret key を import する。
5. `dotfiles gpg export-ssh-public-key` で GPG authentication subkey 由来の SSH public key を出力し、GitHub SSH keys に登録する。
6. `dotfiles secrets restore-pass` で Bitwarden Secrets Manager から `password-store-remote` を取得し、GPG authentication subkey 経由の SSH で private `password-store` repository を clone する。
7. `dotfiles secrets bw-login --email <email>` で Bitwarden Password Manager に login / unlock する。

## コマンド一覧

### `dotfiles secrets yubikey setup`

YubiKey 5 PIV の利用前提を確認し、この機能で使う保存領域が利用可能か確認する。既存の FIDO2 / OTP / OpenPGP / PIV credential は reset しない。既存領域と衝突する場合は停止する。

### `dotfiles secrets yubikey put <name>`

YubiKey に secret を保存する。`<name>` は `bw-password` または `bws-access-token` のみ許可する。secret 本文は hidden prompt または stdin から受け取る。平文を CLI 引数、ログ、一時ファイルに残さない。同名 secret の上書きには明示 option を必要とする。

### `dotfiles secrets yubikey get <name>`

YubiKey から指定 secret を取得する。`<name>` は `bw-password` または `bws-access-token` のみ許可する。YubiKey PIV PIN を要求し、stdout に secret を出力する。通常は他の `dotfiles secrets` コマンド内部で使う。

### `dotfiles secrets restore-gpg`

YubiKey から `bws-access-token` を取得し、Bitwarden Secrets Manager SDK で `gpg-secret-key-backup` を取得する。GPG secret key を import し、encryption / authentication / signing subkey の存在を検証する。最後に `gpg-agent` SSH support が使えることを確認する。

### `dotfiles secrets restore-pass`

YubiKey から `bws-access-token` を取得し、Bitwarden Secrets Manager SDK で `password-store-remote` を取得する。`~/.password-store` が存在しないことを確認し、GPG authentication subkey 経由の SSH で private repository を clone する。clone 後に `pass` が store を読めることを確認する。

### `dotfiles secrets bw-login --email <email>`

YubiKey から `bw-password` を取得し、YubiKey OTP を入力させる。`bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` を実行し、続けて `bw unlock --passwordenv BW_PASSWORD --raw` を実行する。`BW_PASSWORD` は保存しない。`BW_SESSION` の扱いはこのコマンドの設計 PR で確定する。

### `dotfiles gpg export-ssh-public-key`

GPG authentication subkey 由来の SSH public key を stdout に出力する。GitHub SSH keys に登録するために使う。GitHub API は呼ばない。

## API / Command Policy

| 領域 | 使うもの | 使わないもの |
| --- | --- | --- |
| YubiKey | Rust crate | `ykman` CLI、PIV reset、既存 credential 削除 |
| Bitwarden Secrets Manager | 公式 `bitwarden` Rust SDK | 復旧本線での `bw` CLI |
| Bitwarden Password Manager | login / unlock 用の `bw` CLI | secret 取得や永続保存用途の `bw` CLI |
| GnuPG | `gpgme`、必要時の `sequoia-openpgp` | 通常実装での `gpg` CLI、keyserver |
| Git | `git2` + SSH agent | 復旧本線での `git` CLI、GitHub API |

## 停止条件

- YubiKey の専用保存領域が利用できない、または既存 credential と衝突する。
- 許可されていない secret name が指定された。
- 同名 secret が存在し、明示的な上書き option が指定されていない。
- Bitwarden Secrets Manager から必要な secret が取得できない。
- import 対象の GPG secret key に encryption / authentication / signing subkey が揃っていない。
- `gpg-agent` SSH support が利用できない。
- `~/.password-store` が既に存在する。
- `password-store-remote` が private repository の clone URL として妥当でない。
- Bitwarden CLI login / unlock に必要な `bw` CLI、OTP、認証情報が揃っていない。

## 検証

通常のドキュメント変更では static check を実行する。

```sh
cargo xtask check static
```

実装 PR では対象に応じて unit test、fake client によるテスト、manual validation を組み合わせる。YubiKey 実機を使う検証は read-only 確認と専用領域への書き込みに限定し、reset / credential 削除 / 既存領域上書きを含む検証は行わない。

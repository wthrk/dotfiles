# Secret Recovery Spec

復旧入口は YubiKey とユーザー個人の Bitwarden vault で構成する。YubiKey には `bitwarden-client-id` と `bitwarden-client-secret` だけを保存する。`gpg-secret-key-backup` と `password-store-remote` は個人 vault に保存し、dotfiles は vault へのアクセスを CLI ではなく SDK/API adapter 境界で扱う。

YubiKey に保存する Bitwarden 認証材料は account API key の `client_id` / `client_secret` だけである。Bitwarden SDK/API が個人 vault item の復号・作成に必要とする master password は、vault 操作時に CLI/app 側の非表示 input port で取得し、保存しない。管理組織、サービス用アカウント、固定スコープを前提にしない。

## Commands

- `dotfiles secrets yubikey enroll-primary`: 接続中 YubiKey が 1 本だけであることを確認し、CLI/app 側の input port から `bitwarden-client-id` と `bitwarden-client-secret` を受けて保存する。
- `dotfiles secrets yubikey enroll-spare`: spare YubiKey に同じ 2 secret を保存する。primary から bootstrap secret を読み出す経路は持たない。
- `dotfiles secrets yubikey put <bitwarden-client-id|bitwarden-client-secret>`: 低水準保存コマンド。secret 本文は argv では受け取らない。
- `dotfiles secrets yubikey get <bitwarden-client-id|bitwarden-client-secret>`: 低水準取得コマンド。stdout が terminal の場合は平文出力を拒否する。
- `dotfiles secrets verify-yubikey [--check vault|--all]`: local storage と、要求された場合だけ個人 vault adapter 経由の外部確認を行う。
- `dotfiles secrets restore-gpg`: 個人 vault から encrypted envelope を取得し、接続中 YubiKey recipient で復号して GPG secret key を復元する。
- `dotfiles secrets restore-pass`: 個人 vault から `password-store-remote` を取得し、GPG authentication subkey 経由の SSH agent 認証で clone する。
- `dotfiles secrets gpg-backup register`: 既存 envelope が復旧到達条件を満たすか照合する。
- `dotfiles secrets pass-remote register`: `password-store-remote` を個人 vault item として create/use する。

## Handling Rules

shell は client id、client secret、secret、token、API key、URL、fingerprint を argv/stdin/env で中継しない。必要な入力は dotfiles CLI/app 側の TTY/input port で受ける。API key secret 実値、vault secret、URL、fingerprint は log/error/report に出さない。

`--url`、stdin URL、`--primary-fingerprint`、YubiKey serial 指定は提供しない。error chain は潰さず、application 層は support 実装へ直接依存しない。

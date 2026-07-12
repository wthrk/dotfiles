# Bitwarden Secrets Manager Design

復旧情報の外部保存先は Bitwarden Secrets Manager project `dotfiles-secret-recovery` である。dotfiles は SDK/API adapter 境界で BWS secret を取得・作成・更新・照合する。

Bitwarden Secrets Manager の access token は YubiKey storage の `bitwarden-client-secret` に保存する。BWS を読む/書く Rust command は token を hidden prompt / stdin secret input で受け取らず、明示 serial または単一接続で解決した YubiKey から取得する。token 入力が許可されるのは `dotfiles secrets yubikey put bitwarden-client-secret`、`enroll-primary`、`rotate-bws-token` など YubiKey storage へ保存・更新する経路だけである。

## BWS Secrets

- `gpg-secret-key-backup`: GPG secret key backup の encrypted envelope。
- `password-store-remote`: private password-store repository の GitHub SSH clone URL。

URL と envelope 本文は log/error/report に出さない。adapter は外部 API 型と repository domain 型の変換だけを担い、lookup の 0 件・1 件・複数件判定は domain/application 側に置く。

## Provisioning

`pass-remote register` は BWS access token を YubiKey storage から取得し、`password-store-remote` を create または update する。`--url` が指定された場合はその値を使い、未指定の場合だけ CLI/app 側の可視 input port で URL を受ける。URL は credential ではないが private repository の所在を示すため、log/error/report には出さない。

`gpg-backup register` は BWS access token を `--serial` または単一接続で解決した YubiKey storage から取得し、同じ YubiKey の recipient を含む encrypted envelope を `gpg-secret-key-backup` として作成する。`gpg-backup add-spare` は `--unwrap-serial` または単一接続で解決した YubiKey storage から BWS access token を取得し、同じ YubiKey で既存 recipient の DEK を unwrap して spare recipient を追加する。

project 作成は `dotfiles` CLI と script では行わない。BWS command は project `dotfiles-secret-recovery` を名前で解決し、0 件または複数件なら停止する。serial 未指定時の YubiKey 解決は単一接続だけを許可し、複数接続では一覧表示や選択へ進まず fail-closed する。

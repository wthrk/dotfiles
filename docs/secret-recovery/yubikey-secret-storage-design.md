# YubiKey Secret Storage Design

YubiKey PIV storage は復旧入口の bootstrap secret だけを保持する。保存対象は `bitwarden-client-id` と `bitwarden-client-secret` だけである。master password は vault 操作時の非表示 input port で取得し、YubiKey へ保存しない。

## Layout

- manifest object: `0x005FFF16`
- `bitwarden-client-id`: `0x005FFF17`
- `bitwarden-client-secret`: `0x005FFF18`

storage version 1 の secret id は `1 = bitwarden-client-id`、`2 = bitwarden-client-secret` とする。local storage verification は manifest と 2 secret の存在・復号可能性を確認する。

## Commands

`enroll-primary` と `enroll-spare` は single connected YubiKey を要求し、setup を secret 入力前に完了する。secret は CLI/app input port から受け、argv では受け取らない。

`put` と `get` は `bitwarden-client-id` / `bitwarden-client-secret` だけを受け付ける。`get` は terminal stdout への平文出力を拒否する。

`verify-yubikey --check vault` は local storage 検証後、個人 vault adapter 境界で `gpg-secret-key-backup` と `password-store-remote` の到達性を確認する。`--all` は現時点で `vault` check と同義である。

# Bitwarden Personal Vault Design

復旧情報の外部保存先はユーザー個人の Bitwarden vault である。dotfiles は SDK/API adapter 境界で vault item を取得・作成・照合する。

Bitwarden SDK/API の account API key 認証は、個人 vault item を復号・作成するために master password 由来の user crypto 初期化を必要とする。dotfiles は `bitwarden-client-id` / `bitwarden-client-secret` だけを YubiKey に保存し、master password は vault 操作時の CLI/app 非表示 input port から取得して SDK/API adapter 境界へ渡す。shell は master password を argv/stdin/env で中継しない。

## Vault Items

- `gpg-secret-key-backup`: GPG secret key backup の encrypted envelope。
- `password-store-remote`: private password-store repository の GitHub SSH clone URL。

URL と envelope 本文は log/error/report に出さない。adapter は外部 API 型と repository domain 型の変換だけを担い、lookup の 0 件・1 件・複数件判定は domain/application 側に置く。

## Provisioning

`pass-remote register` は configured origin を優先し、origin が無い場合だけ CLI/app 側の controlling TTY input port で URL を受ける。shell から URL を argv/stdin/env で渡さない。

`gpg-backup register` は既存 envelope の primary fingerprint、2 件以上の recipient、connected YubiKey recipient を照合する。新規 envelope は作成せず、`gpg-secret-key-backup` が missing の場合は停止する。初回実運用では source machine script と `enroll-spare` で 2 本以上の YubiKey を準備した後、2 件以上の recipient を含む encrypted envelope を個人 vault item `gpg-secret-key-backup` として投入する。fingerprint や vault secret 実値は出力しない。

初回投入は [initial-provisioning-runbook.md](initial-provisioning-runbook.md) の `gpg-secret-key-backup` envelope 投入手順と gate を正本とする。dotfiles CLI は BWS、Bitwarden Secrets Manager、`bw` CLI login/unlock/session、project、organization を使わず、shell から secret/URL/fingerprint を argv/stdin/env で受け取って envelope を補完しない。gate の監査は `gpg-backup register` が既存 item を読み、primary fingerprint、2 件以上の recipient、connected YubiKey recipient を照合することで行う。YubiKey serial は envelope schema、gate、recipient 照合条件に使わない。

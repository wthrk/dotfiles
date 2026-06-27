# Initial Provisioning Runbook

この runbook は source machine で password-store、GPG、GitHub SSH key、個人 Bitwarden vault の復旧情報を整える手順である。

## Commands

source machine script で primary YubiKey と password-store remote を登録し、spare YubiKey を登録してから、`gpg-secret-key-backup` envelope を個人 Bitwarden vault に投入する。

```sh
bash scripts/provision-secret-recovery-source.sh
dotfiles secrets yubikey enroll-spare
# 手動ステップ（必須・CLI 経路なし）: 下記「`gpg-secret-key-backup` envelope 投入手順」を完了してから次へ進む。
# 個人 Bitwarden vault に 2 recipient 以上の encrypted envelope を投入していない場合、次の register は停止する。
dotfiles secrets gpg-backup register
dotfiles secrets verify-yubikey --all
```

shell script は `client_id`、`client_secret`、URL、fingerprint、API key を argv/stdin/env で受け取らず、dotfiles CLI へも中継しない。必要な入力は dotfiles CLI/app 側の TTY/input port または SDK/API adapter 境界で扱う。

source machine script は `yubikey enroll-primary` を実行し、YubiKey に `bitwarden-client-id` と `bitwarden-client-secret` だけを保存する。その後 `pass-remote register` が YubiKey storage から account API key を読み、master password を非表示 input port で取得して個人 vault を扱う。

`pass-remote register` は configured origin を優先する。origin が無い場合だけ visible TTY input で `password-store-remote` を受ける。`gpg-backup register` は既存 encrypted envelope を照合するだけで、新規 envelope を作成しない。初回実運用では、source machine script と `enroll-spare` の後に、2 件以上の recipient を含む `gpg-secret-key-backup` encrypted envelope を個人 Bitwarden vault に投入し、それから `gpg-backup register` を実行する。envelope 投入が無い場合、`gpg-backup register` は停止する。これは現行 CLI が既存 envelope の primary fingerprint、recipient 数、接続中 YubiKey recipient を検証する経路だけを持つためであり、shell から fingerprint、secret、URL を値付き argv/stdin/env で中継して補完してはならない。

## `gpg-secret-key-backup` envelope 投入手順

source machine script と `dotfiles secrets yubikey enroll-spare` が完了した後、個人 Bitwarden vault に item `gpg-secret-key-backup` を作成し、値として GPG secret key backup の encrypted envelope を保存する。この repository の現行 CLI は envelope 作成・投入経路を提供しないため、投入作業は次の手順と gate を満たす管理済みの envelope preparation 操作として行う。BWS、Bitwarden Secrets Manager、`bw` CLI login/unlock/session、project、organization は使わない。

1. source machine の password-store `.gpg-id` から primary GPG secret key を一意に解決する。raw fingerprint を shell argv/stdin/env、ログ、作業記録へ書かず、envelope metadata にだけ入れる。
2. envelope に含める YubiKey を 1 本ずつ接続し、各 YubiKey の PIV slot `82` public key recipient を envelope preparation 操作内で読み取る。YubiKey serial は schema、gate、照合条件に入れない。recipient 値を shell argv/stdin/env で渡さず、2 本以上が別 recipient であることを確認する。
3. source machine の GPG secret key backup を envelope preparation 操作内で export し、平文 secret key material を shell pipe、argv、stdin、env、永続一時ファイルへ出さない。
4. envelope preparation 操作内で DEK を生成し、GPG secret key backup を暗号化し、2 件以上の recipient で DEK を wrap する。
5. 生成した `GpgBackupEnvelope` JSON を Bitwarden 個人 vault の secure note item `gpg-secret-key-backup` の値として投入する。Bitwarden への認証は個人 account の UI または SDK/API 境界で行い、master password は非表示入力で扱う。envelope 本文は暗号化済み値として vault item にだけ保存し、ログやレビュー記録へ貼らない。
6. source machine に envelope recipient と一致する YubiKey を 1 本接続して `dotfiles secrets gpg-backup register` を実行する。失敗した場合、shell から fingerprint、secret、URL、envelope 本文を補完せず、vault item または envelope preparation 操作を修正して再実行する。

envelope 投入 gate の受け入れ条件は次の通りである。

- 保存先は個人 Bitwarden vault の item 名 `gpg-secret-key-backup` である。
- 値は `GpgBackupEnvelope` JSON であり、`version`、`metadata.primary_fingerprint`、`metadata.exported_at`、`recipients`、`ciphertext` を含む。
- `recipients` には 2 件以上の YubiKey recipient を含める。
- `metadata.primary_fingerprint` は source machine の password-store recipient として解決される primary GPG secret key に対応する。
- connected YubiKey の PIV slot `82` recipient が envelope の `recipients` に含まれる。
- BWS、Bitwarden Secrets Manager、`bw` CLI login/unlock/session、project、organization、shell argv/stdin/env による secret/URL/fingerprint 中継は使わない。
- YubiKey storage へ保存する対象は `bitwarden-client-id` / `bitwarden-client-secret` だけであり、master password、GPG backup envelope、password-store remote は保存しない。

投入後、`dotfiles secrets gpg-backup register` がこの gate を監査する。item が missing、recipient が 1 件だけ、primary fingerprint が source machine の password-store recipient と一致しない、または connected YubiKey recipient が envelope に含まれない場合、command は停止する。失敗時に fingerprint、envelope 本文、URL、secret 値を shell から補完して再実行してはならない。修正は Bitwarden 個人 vault 側の item または envelope preparation 操作で行い、再度 `gpg-backup register` を実行する。

手順側では同じ primary YubiKey に対して低水準 `yubikey put` や `enroll-primary` を重ねて実行しない。

`enroll-primary` / `enroll-spare` は YubiKey に `bitwarden-client-id` と `bitwarden-client-secret` だけを保存する。

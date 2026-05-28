# Bitwarden Secrets Manager 復旧設計

この文書は、[secret-recovery-spec.md](./secret-recovery-spec.md) の [責務分担 / Bitwarden Secrets Manager](./secret-recovery-spec.md#bitwarden-secrets-manager) を具体化する到達設計仕様を定義する恒久文書である。対象は `dotfiles secrets restore-gpg`、`dotfiles secrets restore-pass`、`dotfiles secrets verify-yubikey --check bws` から利用する Bitwarden Secrets Manager 取得経路である。

この文書は完成形の設計だけを扱う。

## 目的と保護境界

この機能の目的は、新規マシン復旧で必要な機械向け secret を Bitwarden Secrets Manager から取得し、必要な復旧処理にだけ受け渡すことである。

保護するもの:

- `bws-access-token` を用いた取得セッション。
- `gpg-secret-key-backup` と `password-store-remote` の取得値。
- 取得値のログ、エラー、診断出力への漏えい。

保護しないもの:

- 復旧処理で import / clone に渡した後の外部ツール内部状態。
- 実行中 host が侵害された状態でのメモリ露出。

## 決定事項

- 復旧本線の取得経路は公式 `bitwarden` Rust SDK を使う。
- `bw` CLI は Bitwarden Secrets Manager 取得経路では使わない。
- `bws-access-token` は YubiKey から取得し、必要な API 呼び出しの範囲だけで保持する。
- `bws-access-token`、`gpg-secret-key-backup`、`password-store-remote` はログ、エラー本文、診断出力に含めない。
- Bitwarden Secrets Manager で扱う secret name は `gpg-secret-key-backup` と `password-store-remote` に固定する。
- `verify-yubikey --check bws` は、上記 2 secret を取得できることを外部確認として検証する。

## secret 取得契約

Bitwarden Secrets Manager で取得する対象と利用先は次のとおり。

- `gpg-secret-key-backup`: `dotfiles secrets restore-gpg` で GPG secret key import 入力に使う。
- `password-store-remote`: `dotfiles secrets restore-pass` で private `password-store` repository clone URL に使う。

取得時は以下を満たす。

- `bws-access-token` が空または取得不能なら即時停止する。
- 必須 secret のいずれかが未登録または取得不能なら停止する。
- 取得値は利用先処理へ直接渡し、恒久保存しない。

## コマンド境界

### `dotfiles secrets restore-gpg`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `gpg-secret-key-backup` を取得する。
- 取得値を GPG import 処理へ渡し、subkey 検証へ進む。

### `dotfiles secrets restore-pass`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `password-store-remote` を取得する。
- clone 前に URL 妥当性と `~/.password-store` 非存在を確認し、clone 処理へ渡す。

### `dotfiles secrets verify-yubikey --check bws`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `gpg-secret-key-backup` と `password-store-remote` の取得可否を確認する。
- 引数なし `verify-yubikey` ではこの外部確認を自動実行せず、状態値 `skipped` を返す。

## 停止条件

- `bws-access-token` が YubiKey から取得できない。
- Bitwarden Secrets Manager SDK の初期化または認証に失敗する。
- `gpg-secret-key-backup` が取得できない。
- `password-store-remote` が取得できない。
- `verify-yubikey --check bws` で外部確認を完了できない。

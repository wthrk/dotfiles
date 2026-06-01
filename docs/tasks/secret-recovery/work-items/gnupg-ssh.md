# #14 GPG 復元 / gpg-agent SSH 対応

- 作業種別: `機能実装`
- 作業目的: `restore-gpg` と `export-ssh-public-key` の経路を、GPG / SSH の外部依存を境界化した形で実装する。
- Design PR 注記: PR #35 は #14 の設計確定であり、Rust 実装完了を意味しない。`restore-gpg` / `export-ssh-public-key` / provisioning の後続実装で未充足契約が残る場合は、本作業項目の実装対象として解消する。
- 構造完了条件:
  - GPG / SSH の実体依存は adapter / port へ閉じる。
  - `application` は復旧順序だけを持つ。
  - `domain` は鍵リング実装や process I/O へ依存しない。
- 既存実装の流用方針: `現行の構成・アーキテクチャを固定の前提とし、既存コードを優先的に流用する。新規追加経路を現行の層境界へ収める範囲で実装し、現行コード構造の大幅な作り替えは前提にしない。`
- 境界維持の観点（新規実装が持ち込んではならない結合）:
  - 外部 crypto / SSH 依存の境界漏れ
  - use case 順序と low-level 操作の結合
  - domain のインフラ依存
- レビュー合格条件: `GnuPG / SSH のインフラ依存が現行の層境界内に収まり、新規実装がアーキテクチャ規約違反を持ち込まないこと。`
- 実装契約（後続 Rust 実装で必須）:
  - `gpg-secret-key-backup` envelope schema 契約を満たすこと。
  - recipient matching（接続中 YubiKey の `yubikey_serial` + `public_key_fingerprint`）を満たすこと。
  - `metadata.primary_fingerprint` は lowercase hex 40 文字（separator なし）として正規化/照合し、実装とテストで検証すること。
  - `verify-yubikey --check bws` の BWS check contract と整合すること（BWS secret 取得可否のみで成功扱いにしない）。

## 現サイクル（Design PR）で確定する必須事項

現サイクルは issue #14 の設計仕様確定を扱う。実装着手前に、次の事項を `docs/secret-recovery/gnupg-ssh-design.md` へ明文化し、未決定項目を残さない。

1. **backup export 入力**: 既存環境の GPG secret key を `gpg-secret-key-backup` 入力として export する方法を決定する。
2. **envelope 化契約**: export 入力を YubiKey recipient 付き encrypted envelope に変換する方式（暗号方式、version、metadata、recipient 形式、YubiKey serial / PIV slot public key fingerprint の扱い）を決定する。
3. **recipient 運用契約**: primary / spare YubiKey の recipient 登録、追加、照合、再暗号化の扱いを決定する。
4. **BWS 登録契約**: encrypted backup envelope の Bitwarden Secrets Manager 登録 / 更新方法と、上書き時の確認・停止条件を決定する。
5. **入出力境界**: export / 暗号化 / 登録 / 復号 / import の各段で、secret key material・data encryption key・復号済み backup を argv / shell history / ログ / 永続一時ファイルへ残さない契約を決定する。
6. **GPG import API**: `restore-gpg` が採用する import API と禁止 API（`gpg` CLI 非採用）を決定する。
7. **復号 + import 契約**: `restore-gpg` が encrypted envelope を取得後、接続中 YubiKey で data encryption key を unwrap して復号済み backup を import へ渡す手順を決定する。
8. **subkey 検証契約**: encryption / authentication / signing subkey の検証条件（利用可能状態を含む）を決定する。
9. **Home Manager gpg-agent.conf**: `gpg-agent` SSH support を Home Manager で管理する設定方針と設定項目を決定する。
10. **zsh 環境変数**: `GPG_TTY` と `SSH_AUTH_SOCK` の設定責務、設定値解決規則、フォールバック方針を決定する。
11. **existing-key stop condition**: 既存鍵リングに同一 key がある場合の停止条件と非対応範囲（上書き非対応）を決定する。

## 完了の判定条件（Design PR）

- `docs/secret-recovery/gnupg-ssh-design.md` に上記すべての項目の決定が記載されている。
- 決定事項が `restore-gpg` / `export-ssh-public-key` / `restore-pass` の境界へ矛盾なく接続されている。
- 停止条件に envelope / recipient 検証失敗、subkey 検証失敗、gpg-agent SSH support 不可、existing-key stop condition が反映されている。

## 後続実装の完了判定条件（#14 Rust 実装）

- Design PR で確定した契約を `restore-gpg` / `export-ssh-public-key` / provisioning 実装へ反映し、未充足項目を残していない。
- envelope schema、recipient matching、primary fingerprint normalization/match、BWS check contract を実装差分とテストで追跡可能に示せる。

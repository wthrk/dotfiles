# #14 GPG 復元 / gpg-agent SSH 対応

- 作業種別: `規約適合リファクタリングを伴う機能実装`
- 作業目的: `restore-gpg` と `export-ssh-public-key` の経路を、GPG / SSH の外部依存を境界化した形で実装する。
- 構造完了条件:
  - GPG / SSH の実体依存は adapter / port へ閉じる。
  - `application` は復旧順序だけを持つ。
  - `domain` は鍵リング実装や process I/O へ依存しない。
- 既存実装の流用方針: `外部依存の境界が曖昧な箇所は流用せず再分割する。`
- 規約違反の解消対象:
  - 外部 crypto / SSH 依存の境界漏れ
  - use case 順序と low-level 操作の結合
  - domain のインフラ依存
- レビュー合格条件: `GnuPG / SSH のインフラ依存が隔離され、アーキテクチャ規約違反が残っていないこと。`

## 現サイクル（Design PR）で確定する必須事項

現サイクルは issue #14 の設計仕様確定を扱う。実装着手前に、次の事項を `docs/secret-recovery/gnupg-ssh-design.md` へ明文化し、未決定項目を残さない。

1. **GPG import API**: `restore-gpg` が採用する import API と禁止 API（`gpg` CLI 非採用）を決定する。
2. **subkey 検証契約**: encryption / authentication / signing subkey の検証条件（利用可能状態を含む）を決定する。
3. **Home Manager gpg-agent.conf**: `gpg-agent` SSH support を Home Manager で管理する設定方針と設定項目を決定する。
4. **zsh 環境変数**: `GPG_TTY` と `SSH_AUTH_SOCK` の設定責務、設定値解決規則、フォールバック方針を決定する。
5. **existing-key stop condition**: 既存鍵リングに同一 key がある場合の停止条件と非対応範囲（上書き非対応）を決定する。

## 完了の判定条件（Design PR）

- `docs/secret-recovery/gnupg-ssh-design.md` に上記 5 項目すべての決定が記載されている。
- 決定事項が `restore-gpg` / `export-ssh-public-key` / `restore-pass` の境界へ矛盾なく接続されている。
- 停止条件に subkey 検証失敗、gpg-agent SSH support 不可、existing-key stop condition が反映されている。

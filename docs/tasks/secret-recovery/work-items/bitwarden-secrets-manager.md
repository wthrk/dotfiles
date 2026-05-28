# #13 Bitwarden Secrets Manager クライアント

- 作業種別: `規約適合リファクタリングを伴う機能実装`
- 作業目的: `Bitwarden Secrets Manager` 取得経路を、secret-recovery の層分割と外部境界規約に沿って実装する。
- 現行サイクル確認基準: `この current-cycle 実装/テスト差分終端 HEAD までの差分`
- 実装/テスト差分の保存コミット終端: `この current-cycle 実装/テスト差分終端 HEAD`（自己 hash は本文へ埋め込まず、git log の HEAD で確認する）
- 構造完了条件:
  - SDK 呼び出しは adapter / port 境界へ隔離する。
  - `application` は secret recovery の順序制御だけを持つ。
  - `domain` は Bitwarden SDK 型と I/O 型へ依存しない。
- 既存実装の流用方針: `規約に合う部分だけを流用し、境界違反が残る場合は再分割を優先する。`
- 規約違反の解消対象:
  - SDK 依存の境界漏れ
  - application と domain の責務混在
  - CLI / adapter / domain の直接結合
- レビュー合格条件: `Bitwarden SDK 依存が境界内へ閉じ、アーキテクチャ規約違反が残っていないこと。`

## 完了の判定条件（監査再現）

- 注記: 現サイクルはデザインPR段階（確認証跡・レビュー証跡の是正中）であり、本節は完了判定時に満たすべき監査条件を定義する。現時点で完了判定を意味しない。
- 本項目先頭の固定記録フィールド `現行サイクル確認基準` と `実装/テスト差分の保存コミット終端` が同一 `current-cycle` 記録として維持され、同一差分の再特定に使えること。
- [`../review-artifacts/bitwarden-secrets-manager/confirmation.md`](../review-artifacts/bitwarden-secrets-manager/confirmation.md) の確認記録（`verify-yubikey --check bws` を含む）が、`現行サイクル確認基準` に対する実行範囲として追跡可能であること。
- [`../review-artifacts/bitwarden-secrets-manager/review.md`](../review-artifacts/bitwarden-secrets-manager/review.md) に、現サイクル（デザインPR段階の文書是正）で必須の `運用整合レビュー担当` と `参照整合レビュー担当` の個別判定および `集約後レビュー判定: 合格` が揃い、`実装/テスト差分の保存コミット終端` に対する監査補助記録として追跡可能であること。

# #13 Bitwarden Secrets Manager クライアント

- 作業種別: `規約適合リファクタリングを伴う機能実装`
- 作業目的: `Bitwarden Secrets Manager` 取得経路を、secret-recovery の層分割と外部境界規約に沿って実装する。
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

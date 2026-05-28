# #16 Bitwarden Password Manager CLI ログイン

- 作業種別: `規約適合リファクタリングを伴う機能実装`
- 作業目的: `bw-login` 経路を、CLI、application、外部 command 境界の役割分担に従って実装する。
- 構造完了条件:
  - `bw` CLI 呼び出しは adapter / port 境界へ閉じる。
  - YubiKey 由来 secret の取得順序は application が持つ。
  - `domain` は `bw` CLI や process 実行詳細に依存しない。
- 既存実装の流用方針: `既存フローを参考にしてよいが、責務境界に違反する実装は保持しない。`
- 規約違反の解消対象:
  - process 実行の境界漏れ
  - secret 入出力境界の混在
  - use case 順序と外部 command 詳細の結合
- 規約文書更新成果物: [docs/secret-recovery/bitwarden-password-manager-design.md](../../../secret-recovery/bitwarden-password-manager-design.md)
- レビュー合格条件: `外部 command 依存が境界内へ閉じ、アーキテクチャ規約違反が残っていないこと。`

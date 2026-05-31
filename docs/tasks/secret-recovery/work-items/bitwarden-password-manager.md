# #16 Bitwarden Password Manager CLI ログイン

- 作業種別: `機能実装`
- 作業目的: `bw-login` 経路を、CLI、application、外部 command 境界の役割分担に従って実装する。
- 構造完了条件:
  - `bw` CLI 呼び出しは adapter / port 境界へ閉じる。
  - YubiKey 由来 secret の取得順序は application が持つ。
  - `domain` は `bw` CLI や process 実行詳細に依存しない。
- 既存実装の流用方針: `現行の構成・アーキテクチャを固定の前提とし、既存フロー・既存コードを優先的に流用する。新規追加経路を現行の層境界へ収める範囲で実装し、現行コード構造の大幅な作り替えは前提にしない。`
- 境界維持の観点（新規実装が持ち込んではならない結合）:
  - process 実行の境界漏れ
  - secret 入出力境界の混在
  - use case 順序と外部 command 詳細の結合
- レビュー合格条件: `外部 command 依存が現行の層境界内に収まり、新規実装がアーキテクチャ規約違反を持ち込まないこと。`

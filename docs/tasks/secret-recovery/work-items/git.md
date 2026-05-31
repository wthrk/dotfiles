# #15 password-store 復元

- 作業種別: `機能実装`
- 作業目的: `restore-pass` 経路を、Git、SSH agent、remote URL 取得の境界を分離した形で実装する。
- 構造完了条件:
  - Git 操作は adapter / port 境界へ閉じる。
  - `application` は restore 手順と停止条件だけを持つ。
  - `domain` は Git 実装、SSH agent 実装、filesystem 実装に依存しない。
- 既存実装の流用方針: `現行の構成・アーキテクチャを固定の前提とし、既存コードを優先的に流用する。新規追加経路を現行の層境界へ収める範囲で実装し、現行コード構造の大幅な作り替えは前提にしない。`
- 境界維持の観点（新規実装が持ち込んではならない結合）:
  - Git / filesystem / SSH agent 依存の混在
  - restore 手順と low-level clone 詳細の結合
  - domain のインフラ依存
- レビュー合格条件: `Git 復旧経路が現行の層境界内に収まり、新規実装がアーキテクチャ規約違反を持ち込まないこと。`

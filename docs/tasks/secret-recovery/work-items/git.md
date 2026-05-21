# #15 password-store 復元

- 作業種別: `規約適合リファクタリングを伴う機能実装`
- 作業目的: `restore-pass` 経路を、Git、SSH agent、remote URL 取得の境界を分離した形で実装する。
- 構造完了条件:
  - Git 操作は adapter / port 境界へ閉じる。
  - `application` は restore 手順と停止条件だけを持つ。
  - `domain` は Git 実装、SSH agent 実装、filesystem 実装に依存しない。
- 既存実装の流用方針: `既存コードは境界規約に適合する単位だけ流用し、そうでない場合は再分割を優先する。`
- 規約違反の解消対象:
  - Git / filesystem / SSH agent 依存の混在
  - restore 手順と low-level clone 詳細の結合
  - domain のインフラ依存
- レビュー合格条件: `Git 復旧経路の境界分離が完了し、アーキテクチャ規約違反が残っていないこと。`

# docs

このディレクトリは、本リポジトリの文書群の入口である。

## 配下の項目

- [architecture/README.md](architecture/README.md): 実装アーキテクチャ規約の文書群を案内する。
- [secret-recovery/README.md](secret-recovery/README.md): 秘密情報復旧機能の文書群を案内する。
- [docs-governance.md](docs-governance.md): 文書運用規約を定義する。

## タスク管理の参照順

- タスク管理の解釈規則（進行依頼全般）は [docs-governance.md](docs-governance.md) を正本として参照する。
- secret-recovery の運用時は [secret-recovery/tasks.md](secret-recovery/tasks.md) を固定実装単位と作業項目の進捗正本として参照する。
- secret-recovery の Phase/Issue レベルの完了判定は [secret-recovery/implementation-guidelines.md](secret-recovery/implementation-guidelines.md) の判定規則を正本とし、`tasks.md` の完了表示だけで代替しない。

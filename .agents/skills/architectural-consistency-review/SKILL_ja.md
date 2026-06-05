---
name: architectural-consistency-review
description: アーキテクチャ整合レビュー担当として判定するときに使う。
---

# Architectural Consistency Review

## Actor Binding

このスキル有効時の現在アクターは **architectural-consistency reviewer**。

## Governing Sources

- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/task-governance/implementation-review-judgement.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/architecture/hexagonal-implementation-rules.md`
5. ユーザー指定の GitHub issue、PR、明示タスク、または委譲されたレビュー入力
6. 入力が要求する追加正本文書

## Rules

- この役割の判定だけを行い、ソース編集、コミット、別役割の作業をしない。
- 対象コード、文書、issue、PR、task を直接読む。過去記録、要約、実装担当報告で判定を代替しない。
- この役割の境界は `docs/task-governance/implementation-review-judgement.md` に従う。構造チェックリストの合否総和ではなく、モジュール全体の設計整合を判定する。
- この役割の governing source を適用し、詳細規則をここで再掲しない。
- reviewer として動作する場合は `docs/task-governance/implementation-review-judgement.md` が要求する verdict 形式で返す。

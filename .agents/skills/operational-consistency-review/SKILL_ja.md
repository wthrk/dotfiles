---
name: operational-consistency-review
description: 運用整合レビュー担当として判定するときに使う。
---

# Operational Consistency Review

## Actor Binding

このスキル有効時の現在アクターは **operational-consistency reviewer**。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`
- `docs/docs-governance.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/docs-governance.md`
5. ユーザー指定の GitHub issue、PR、明示タスク、または委譲されたレビュー入力
6. 入力が要求する追加正本文書

## Rules

- この役割の判定だけを行い、ソース編集、コミット、別役割の作業をしない。
- 対象コード、文書、issue、PR、task を直接読む。過去記録、要約、実装担当報告で判定を代替しない。
- workflow 手順、役割分離、gate 条件、commit/PR 運用文書がレビュー対象である場合を含め、この役割の境界は `docs/task-governance/implementation-review-judgement.md` に従う。
- この役割の governing source を適用し、詳細規則をここで再掲しない。
- reviewer として動作する場合は `docs/task-governance/implementation-review-judgement.md` が要求する verdict 形式で返す。

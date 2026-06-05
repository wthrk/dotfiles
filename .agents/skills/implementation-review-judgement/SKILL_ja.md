---
name: implementation-review-judgement
description: レビュー開始可否と必須レビュー担当の判定集約を行うときに使う。
---

# Implementation Review Judgement

## Actor Binding

このスキル有効時の現在アクターは **review aggregation judge**。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. ユーザー指定の GitHub issue、PR、明示タスク、または委譲されたレビュー入力
5. 入力が要求する追加正本文書

## Rules

- review aggregation judge としての判定だけを行い、ソース編集、コミット、個別 reviewer としての判定、別役割の作業をしない。
- 対象コード、文書、issue、PR、task を直接読む。過去記録、要約、実装担当報告で判定を代替しない。
- この役割の governing source を適用し、詳細規則をここで再掲しない。
- `docs/task-governance/implementation-review-judgement.md` が要求する集約判定形式で返す。

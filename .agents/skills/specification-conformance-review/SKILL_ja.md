---
name: specification-conformance-review
description: 仕様適合レビュー担当として判定するときに使う。
---

# Specification Conformance Review

## Actor Binding

このスキル有効時の現在アクターは **specification-conformance reviewer**。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`
- 委譲入力が要求するユーザー指定 GitHub issue、PR、明示タスク、および領域固有仕様

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. ユーザー指定の GitHub issue、PR、明示タスク、または委譲されたレビュー入力
5. 入力が要求する追加正本文書

## Rules

- この役割の判定だけを行い、ソース編集、コミット、別役割の作業をしない。
- 対象コード、文書、issue、PR、task を直接読む。過去記録、要約、実装担当報告で判定を代替しない。
- 判定前に、委譲入力が要求する正本仕様と完了条件を特定して読む。
- この役割の governing source を適用し、詳細規則をここで再掲しない。
- reviewer として動作する場合は `docs/task-governance/implementation-review-judgement.md` が要求する verdict 形式で返す。

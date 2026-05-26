---
name: security-review
description: セキュリティレビュー担当として判定するときに使う。
---

# Security Review

## Actor Binding

このスキル有効時の現在アクターは **security reviewer**。

## Governing Sources

- `docs/task-governance/security-obligations.md`
- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/workflow.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/security-obligations.md`
5. `docs/task-governance/implementation-review-judgement.md`
6. 委譲されたレビュー対象パス

## Rules

- 機密露出・不正アクセス経路・権限昇格リスクを評価する。
- 要約で代替せず、実コードを直接確認する。
- 判定返却のみ行い、実装編集はしない。
- 判定規則は正本参照に従い、このファイルで重複定義しない。

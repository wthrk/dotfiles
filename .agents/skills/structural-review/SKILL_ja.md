---
name: structural-review
description: 層責務・依存方向・可視性の構造レビュー担当として判定するときに使う。
---

# Structural Review

## Actor Binding

このスキル有効時の現在アクターは **structural reviewer**。

## Governing Sources

- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/architecture/review-checklist.md`
- `docs/task-governance/implementation-review-judgement.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/architecture/hexagonal-implementation-rules.md`
4. `docs/architecture/review-checklist.md`
5. `docs/task-governance/implementation-review-judgement.md`

## Rules

- 層責務・依存・可視性の確認は正本参照に従う。
- 判定根拠は実コード直接確認で記録する。
- 判定返却のみ行い、実装編集はしない。
- チェック項目をこのファイルで重複定義しない。

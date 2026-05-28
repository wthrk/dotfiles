---
name: architectural-consistency-review
description: モジュール全体整合を判定するアーキテクチャ整合レビュー担当として使う。
---

# Architectural Consistency Review

## Actor Binding

このスキル有効時の現在アクターは **architectural-consistency reviewer**。

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

- シンボル単位ではなく、モジュール全体の設計整合を判定する。
- 対象モジュールを独立に直接読んで判定する。
- 判定返却のみ行い、実装編集はしない。
- 規範詳細はこのファイルで再定義しない。

---
name: operational-consistency-review
description: 運用整合レビュー担当として判定するときに使う。
---

# Operational Consistency Review

## Actor Binding

このスキル有効時の現在アクターは **operational-consistency reviewer**。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/workflow.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/tasks/README.md`
6. `docs/tasks/tasks.md`
7. active work item が要求する領域資料（`docs/tasks/<area>/...`）
8. 関連する `docs/tasks/<area>/review-artifacts/...`

`docs/tasks/<area>/tasks.md` は、active work item が明示的に参照している場合だけ必須とする。

## Rules

- 実行手順とゲートの強制可能性・監査可能性を判定する。
- review artifact、confirmation、台帳、current-cycle 文言、exact file-set、文書-only 更新 hash の完全同期を運用ゲートにしない。対象差分、必要な実検証、必須レビュー結果、コミット連動判定が識別できれば足りる。補助記録は監査補助であり、完全同期だけで合否を決めない。
- 判定返却のみ行い、実装編集はしない。
- レビュー独立性は正本参照に従う。
- このファイルは簡潔に保ち、正本依存とする。

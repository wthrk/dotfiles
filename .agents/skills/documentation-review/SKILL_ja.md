---
name: documentation-review
description: コード doc comment のドキュメントレビュー担当として判定するときに使う。
---

# Documentation Review

## Actor Binding

このスキル有効時の現在アクターは **documentation reviewer**。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`（ドキュメントレビュー担当の職責）
- `docs/architecture/hexagonal-implementation-rules.md`（ドキュメントコメント規則）
- `docs/docs-governance.md`（存在する場合）

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/architecture/hexagonal-implementation-rules.md`
5. `docs/docs-governance.md`（存在する場合）

## Rules

- 実装とコード内 doc comment の整合を判定する。
- production 必須対象と test case 除外を含む判定範囲は正本参照に従う。
- 判定返却のみ行い、実装編集はしない。
- このファイルで独自規範を追加しない。

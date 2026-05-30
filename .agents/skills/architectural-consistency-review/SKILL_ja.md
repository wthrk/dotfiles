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
- 機械的分離ではなく、処理が正本アーキテクチャ文書で規定された責務境界に置かれた一貫した設計かを判定する。
- 薄い port / adapter を作るために、正本アーキテクチャ文書が他層へ割り当てる責務を別層へ逃がしていないかを確認する。
- adapter 配下の `secrets-internal-test-stub` feature 専用 backend stub は、正本の internal backend stub 条件に照らして判定する。その存在だけで全体非整合と判定しない。
- 対象モジュールを独立に直接読んで判定する。
- 判定返却のみ行い、実装編集はしない。
- 規範詳細はこのファイルで再定義しない。

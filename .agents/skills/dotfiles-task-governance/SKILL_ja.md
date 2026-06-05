---
name: dotfiles-task-governance
description: 役割確定済みの実行者が、dotfiles または secret-recovery 固有の統治制約を正本文書から適用するときに使う。
---

# Dotfiles Task Governance

## Actor Binding

このスキルは現在アクターの役割を確立・変更しない。現在アクターは、すでに起動済みの役割スキルに拘束され続ける。

## Governing Sources

- `docs/task-governance/security-obligations.md`
- `docs/task-governance/pr-mergeability-loop.md`
- `docs/secret-recovery/implementation-guidelines.md`
- `docs/docs-governance.md`

## Required Reading Order

1. 現在アクターを確定した役割スキル
2. `docs/task-governance/README.md`
3. `docs/task-governance/security-obligations.md`
4. main orchestrator が PR URL、PR 番号、または確立済み文脈で対象 PR を特定できる依頼として、mergeability 関連操作、PR review 対応、PR 文脈の AI/Codex/Copilot review、PR 文脈の `@codex review`、review thread resolve、PR 文脈の checks 確認、PR 文脈の mergeability 確認、または明示的に PR mergeability に関係する依頼を扱う場合は `docs/task-governance/pr-mergeability-loop.md`
5. `docs/secret-recovery/implementation-guidelines.md`
6. `docs/docs-governance.md`
7. 統治対象領域の正本文書

## Rules

- リポジトリ固有の統治補助としてだけ使う。
- 委譲済み役割アクターが同じ delegated task のオーケストレーターへ自己切替するために使わない。
- top-level の PR 指示で、PR URL、PR 番号、または確立済み文脈によって対象 PR を特定でき、mergeability 関連操作、PR review 対応、PR 文脈の AI/Codex/Copilot review、PR 文脈の `@codex review`、review thread resolve、PR 文脈の checks 確認、PR 文脈の mergeability 確認、または明示的に PR mergeability に関係する依頼がある場合、main orchestrator は `/pr-mergeability-loop` もオーケストレーション拡張として使う。
- 委譲済み actor は `/pr-mergeability-loop` を使って PR loop 全体を引き取らない。割り当てられた役割に従い、PR-loop に関係する事実を親オーケストレーターへ報告する。
- 正本文書を直接適用する。このスキルは詳細規則を再掲しない。

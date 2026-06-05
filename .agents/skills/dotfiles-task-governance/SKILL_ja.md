---
name: dotfiles-task-governance
description: 役割確定済みの実行者が、dotfiles または secret-recovery 固有の統治制約を正本文書から適用するときに使う。
---

# Dotfiles Task Governance

## Actor Binding

このスキルは現在アクターの役割を確立・変更しない。現在アクターは、すでに起動済みの役割スキルに拘束され続ける。

## Governing Sources

- `docs/task-governance/security-obligations.md`
- `docs/secret-recovery/implementation-guidelines.md`
- `docs/docs-governance.md`

## Required Reading Order

1. 現在アクターを確定した役割スキル
2. `docs/task-governance/README.md`
3. `docs/task-governance/security-obligations.md`
4. `docs/secret-recovery/implementation-guidelines.md`
5. `docs/docs-governance.md`
6. 統治対象領域の正本文書

## Rules

- リポジトリ固有の統治補助としてだけ使う。
- 委譲済み役割アクターが同じ delegated task のオーケストレーターへ自己切替するために使わない。
- 正本文書を直接適用する。このスキルは詳細規則を再掲しない。

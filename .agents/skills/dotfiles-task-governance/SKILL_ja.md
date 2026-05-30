---
name: dotfiles-task-governance
description: dotfiles リポジトリ固有または secret-recovery 固有の統治制約を、正本文書から適用する必要がある場合に orchestration と併用する。
---

# Dotfiles Task Governance

## 正本

- `docs/task-governance/workflow.md`
- `docs/task-governance/implementation-execution.md`
- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/task-completion-judgement.md`
- `docs/task-governance/security-obligations.md`
- `docs/secret-recovery/implementation-guidelines.md`
- `docs/docs-governance.md`

## 必須参照順

この順序は導線だけを示す。詳細規則は役割スキルと正本文書が所有する。

1. 現在の実行者の役割を確定した役割スキル
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. 現在の役割または gate に対応する task-governance 文書
5. `docs/task-governance/security-obligations.md`
6. secret-recovery が対象の場合は `docs/secret-recovery/implementation-guidelines.md`
7. 文書配置または正本扱いが対象の場合は `docs/docs-governance.md`

## 使用時

このスキルは、適用される役割スキルによって役割が確定済みの実行者が、リポジトリ固有の統治補助を必要とする場合にだけ使う。

このスキルは現在の実行者の役割を確立・変更しない。現在の実行者は、すでに起動済みの `/orchestration`、`/implementation-execution`、レビュー系スキル、判定系スキルなどの役割スキルに拘束され続ける。

このスキルは `/orchestration`、`/implementation-execution`、レビュー系スキル、判定系スキルを置き換えない。委譲された役割エージェントは、このスキルを使って同じ delegated task のオーケストレーターへ自己切替してはならない。

## 規則

リポジトリ固有のタスクフロー、役割分離、レビューゲート、完了ゲート、証跡管理、セキュリティ義務、secret-recovery 制約は、上記の正本文書が所有する。このスキルは、それらの詳細規則を再掲・再解釈しない。

統治対象領域で行動する前に、その領域の正本を読み、正本を直接適用する。このスキルと正本の間に矛盾がある場合は、この要約ではなく正本文書に従う。

---
name: orchestration
description: このリポジトリの top-level タスク実行依頼で、メインエージェントが active item 選定と役割委譲を統治規約に従って行う場合に使う。
---

# Orchestration

## 正本

- `docs/README.md`
- `docs/tasks/README.md`
- `docs/tasks/tasks.md`
- `docs/task-governance/workflow.md`
- `docs/docs-governance.md`

## 必須参照順

この順序は導線だけを示す。詳細規則は正本文書が所有する。

1. `docs/README.md`
2. `docs/tasks/README.md`
3. `docs/tasks/tasks.md`
4. `docs/task-governance/README.md`
5. `docs/task-governance/workflow.md`
6. `docs/task-governance/workflow.md` が要求する active work item 参照先
7. 文書配置または正本扱いが対象の場合は `docs/docs-governance.md`

## 使用時

このリポジトリの top-level タスク実行依頼で、メインエージェントがオーケストレーターとして動作している間だけ、このスキルを使う。

委譲された実装担当、レビュー担当、進捗判定担当、完了判定担当は、同じ delegated task についてオーケストレーターにはならない。割り当てられた役割スキルを使い、委譲された役割を直接実行する。

## 規則

タスクフロー、active item 選定、オーケストレーターの許可行為、役割分離、委譲義務、実行機構に依存しない役割扱い、失敗時処理の正本は `docs/task-governance/workflow.md` である。このスキルは、それらの詳細規則を再掲・再解釈しない。

オーケストレーターとして行動する前に、`docs/task-governance/workflow.md` が定める入口手順と active item / 委譲フローに従い、`docs/README.md`、`docs/tasks/README.md`、`docs/tasks/tasks.md` をリポジトリ入口として使う。

このスキルと正本の間に矛盾がある場合は、この要約ではなく正本文書に従う。

---
name: task-completion-judgement
description: 作業単位を完了扱いにできるか判定するときに使う。
---

# Task Completion Judgement

## Actor Binding

このスキル有効時の現在アクターは **task-completion judge**。

## Governing Sources

- `docs/task-governance/task-completion-judgement.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/task-completion-judgement.md`
4. ユーザー指定の GitHub issue、PR、明示タスク、または委譲されたレビュー入力
5. 入力が要求する追加正本文書

## Rules

- task-completion judge としての判定だけを行い、ソース編集、コミット、reviewer としての判定、別役割の作業をしない。
- 対象コード、文書、issue、PR、task を直接読む。過去記録、要約、実装担当報告で判定を代替しない。
- この役割の governing source を適用し、詳細規則をここで再掲しない。
- `docs/task-governance/task-completion-judgement.md` が要求する完了判定を返す。

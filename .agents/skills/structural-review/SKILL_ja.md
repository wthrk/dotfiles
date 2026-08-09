---
name: structural-review
description: 構造レビュー担当として判定するときに使う。
---

# Structural Review

## Actor Binding

このスキル有効時の現在アクターは **structural reviewer**。

## Governing Sources

- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/task-governance/implementation-review-judgement.md`
- `docs/docs-governance.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/architecture/hexagonal-implementation-rules.md`
5. `docs/docs-governance.md`
6. ユーザー指定の GitHub issue、PR、明示タスク、または委譲されたレビュー入力
7. 入力が要求する追加正本文書

## Rules

- この役割の判定だけを行い、ソース編集、コミット、別役割の作業をしない。
- 対象コード、文書、issue、PR、task を直接読む。過去記録、要約、実装担当報告で判定を代替しない。
- 検査の新設や拡張を求める finding を出す前に、それが `docs/docs-governance.md` の禁止する検査形式に当たらないかを確かめる。当たるなら finding にせず、その形式の検査が既にあるなら削除を求める。
- この役割の governing source を適用し、詳細規則をここで再掲しない。
- reviewer として動作する場合は `docs/task-governance/implementation-review-judgement.md` が要求する verdict 形式で返す。

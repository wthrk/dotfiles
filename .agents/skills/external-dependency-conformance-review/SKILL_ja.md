---
name: external-dependency-conformance-review
description: 外部依存適合レビュー担当として判定するときに使う。
---

# External Dependency Conformance Review

## Actor Binding

このスキル有効時の現在アクターは **external-dependency-conformance reviewer**。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`
- 委譲入力が要求する依存先の context7 documentation または現行公式文書

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. ユーザー指定の GitHub issue、PR、明示タスク、または委譲されたレビュー入力
5. 入力が要求する依存先の context7 documentation または現行公式文書
6. 入力が要求する追加正本文書

## Rules

- この役割の判定だけを行い、ソース編集、コミット、別役割の作業をしない。
- 対象コード、文書、issue、PR、task を直接読む。過去記録、要約、実装担当報告で判定を代替しない。
- 委譲入力が対象にする重要な外部 SDK / crate を特定し、利用可能ならその current context7 documentation を、なければ現行公式文書を読む。
- 既知知識だけで推定せず、文書化された API・挙動・制約に対する適合として判定する。
- reviewer として動作する場合は `docs/task-governance/implementation-review-judgement.md` が要求する verdict 形式で返す。

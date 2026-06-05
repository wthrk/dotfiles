---
name: pr-mergeability-loop
description: PR review 対応、AI/Codex/Copilot review、@codex review、review thread resolve、checks、mergeability 確認を含む PR 指示で補助スキルとして使う。
---

# PR Mergeability Loop

## Actor Binding

このスキルは補助スキルのみである。現在アクターの役割を確立・変更しない。

現在アクターは、すでに起動済みの `/implementation-execution`、レビュー系スキル、判定系スキルなどの役割スキルに拘束され続ける。委譲済み役割アクターは、このスキルを使って同じ delegated task の再オーケストレーション、作業単位の再選定、subagent 起動を行ってはならない。

## Governing Sources

- `docs/task-governance/pr-mergeability-loop.md`
- `docs/task-governance/workflow.md`、特に PR 運用規則
- 文書配置または正本扱いが対象の場合は `docs/docs-governance.md`

## Required Reading Order

1. 現在アクターを確定した役割スキル
2. `docs/task-governance/pr-mergeability-loop.md`
3. `docs/task-governance/workflow.md` の PR 運用規則
4. 文書配置または正本扱いが対象の場合は `docs/docs-governance.md`

## When To Use

指示に次のいずれかが含まれる場合、現在の役割と併用する。

- PR URL または PR 番号
- PR review 対応
- AI review、Codex review、Copilot review、または `@codex review`
- review thread の返信または resolve
- checks 確認
- mergeability 確認

## Rules

- `docs/task-governance/pr-mergeability-loop.md` を直接適用する。
- ゴールは PR を確認可能な merge 可能状態にすることであり、実際の merge 実行ではない。
- 最新 PR head OID を対象にし、修正 push のたびに反復する。
- 完了報告前に、checks、mergeability、未解決 review thread、最新 head に対する AI/Codex/Copilot review 状態を確認する。
- 採用または不採用の review comment には必要な返信を行い、権限がある場合は対応済み thread を resolve する。
- checks pending/failing、外部 review 未完了、resolve 権限不足、conflict、branch protection 未充足、最新 head の AI/Codex/Copilot review 取得不能がある場合は、完了扱いにせず保留条件として報告する。

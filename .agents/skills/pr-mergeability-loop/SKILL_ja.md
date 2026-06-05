---
name: pr-mergeability-loop
description: PR review 対応、AI/Codex/Copilot review、PR 文脈の @codex review、review thread resolve、checks、mergeability 確認を含む PR 指示で補助スキルとして使う。
---

# PR Mergeability Loop

## Actor Binding

このスキルは補助スキルのみである。現在アクターの役割を確立・変更しない。

現在アクターは、すでに起動済みの `/implementation-execution`、レビュー系スキル、判定系スキルなどの役割スキルに拘束され続ける。委譲済み役割アクターは、このスキルを使って同じ delegated task の再オーケストレーション、作業単位の再選定、subagent 起動を行ってはならない。

## Governing Sources

- `docs/task-governance/pr-mergeability-loop.md`
- `docs/task-governance/workflow.md`、特に `## 8. ブランチ・コミット・プルリクエスト運用`
- 文書配置または正本扱いが対象の場合は `docs/docs-governance.md`

## Required Reading Order

1. 現在アクターを確定した役割スキル
2. `docs/task-governance/pr-mergeability-loop.md`
3. `docs/task-governance/workflow.md` の `## 8. ブランチ・コミット・プルリクエスト運用`
4. 文書配置または正本扱いが対象の場合は `docs/docs-governance.md`

## When To Use

指示に次のいずれかが含まれる場合、現在の役割と併用する。

- PR URL または PR 番号と、mergeability、checks、review thread、PR review 対応に関する依頼
- PR review 対応
- PR 文脈の AI review、Codex review、Copilot review、または `@codex review`
- review thread の返信または resolve
- PR 文脈の checks 確認
- mergeability 確認

## Rules

- `docs/task-governance/pr-mergeability-loop.md` を直接適用する。このスキルは永続的な反復手順を再掲しない。
- すでに確立済みの現在アクターの役割と権限の範囲内でのみ使う。
- 現在役割で実行できない操作は、この補助スキルを根拠に実行せず、実行不能事項として報告する。

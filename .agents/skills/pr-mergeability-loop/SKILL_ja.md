---
name: pr-mergeability-loop
description: PR review 対応、AI/Codex/Copilot review、PR 文脈の @codex review、review thread resolve、PR 文脈の checks、PR 文脈の mergeability 確認を含む PR 指示で、main orchestrator のオーケストレーション拡張として使う。
---

# PR Mergeability Loop

## Actor Binding

このスキルは main orchestrator のためのオーケストレーション拡張用補助スキルである。現在アクターの役割を確立・変更せず、独立した delegated PR-loop 役割を作らない。

top-level の PR マージ可能化依頼では、main orchestrator が `/orchestration` と併用してループを調整する。委譲済みの実装担当、レビュー担当、判定担当、commit / PR 操作担当は、このスキルを使ってループ全体を引き取ったり、同じ delegated task の再オーケストレーション、作業単位の再選定、subagent 起動を行ったりしてはならない。各 delegated actor は割り当てられた役割に拘束され続け、PR-loop に関係する事実を親オーケストレーターへ報告する。

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

top-level 指示に次のいずれかが含まれる場合、main orchestrator がオーケストレーション拡張として使う。

- PR URL または PR 番号と、PR 文脈の checks 確認、PR 文脈の mergeability 確認、review thread 対応、PR review 対応、AI/Codex/Copilot review 対応、thread resolve など、PR mergeability に関係する操作または依頼
- PR review 対応
- PR 文脈の AI review、Codex review、Copilot review、または `@codex review`
- review thread の返信または resolve
- PR 文脈の checks 確認
- PR 文脈の mergeability 確認

## Rules

- `docs/task-governance/pr-mergeability-loop.md` を直接適用する。このスキルは永続的な反復手順を再掲しない。
- このスキルは PR マージ可能化依頼に対して `/orchestration` を拡張する。`/orchestration`、`/implementation-execution`、レビュー系スキル、判定系スキル、または `workflow.md` の commit / PR 操作規則を置き換えない。
- main orchestrator は、対象 PR の確定、PR 状態の inventory、bounded delegation、結果集約、完了または保留条件が判明するまでの再確認を調整する。
- 委譲済み actor は、割り当てられた bounded task について該当する役割スキルを使い、checks、review thread、review 結果、commit / push、PR 操作に関する事実を親オーケストレーターへ返す。AI / PR loop 全体を所有しない。

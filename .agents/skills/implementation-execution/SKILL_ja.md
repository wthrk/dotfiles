---
name: implementation-execution
description: サブエージェントが実装作業を割り当てられ、リポジトリの実行義務に従って diff を作成するときに使う。
---

# Implementation Execution

## Actor Binding

このスキル有効時の現在アクターは **implementation executor**。

## Governing Sources

- `docs/task-governance/implementation-execution.md`
- `docs/task-governance/security-obligations.md`
- `docs/architecture/hexagonal-implementation-rules.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-execution.md`
4. `docs/task-governance/security-obligations.md`
5. `docs/architecture/hexagonal-implementation-rules.md`
6. 委譲された GitHub issue、PR、明示タスク、または handoff
7. 委譲作業が要求する領域正本文書

## Rules

- delegated task の orchestration は完了済みとして扱う。
- 同じ delegated task について `/orchestration` を起動せず、`$dotfiles-task-governance` を orchestration、役割変更、作業単位の再選定に使わない。
- 作業単位を再選定せず、同じ実装割当に対して subagent を起動しない。
- 編集前に対象ファイル、直接依存、呼び出し元/呼び出し先、テスト、handoff finding を読む。
- 割り当て差分を作成し、選択した検証を実行し、対象差分・コマンド・結果・未実施確認・残リスクを報告する。
- 詳細な実装義務は `docs/task-governance/implementation-execution.md` が所有する。

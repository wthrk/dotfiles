---
name: orchestration
description: Use this skill whenever a task-execution instruction is received — including file creation, implementation, review, or completion judgement — to orchestrate work via active-item selection and subagent delegation.
---

# Orchestration

## Absolute Prohibitions (read this before anything else)

The following are unconditionally forbidden for the orchestrator. No exception exists.

1. **Do not read files outside the Required Reading Order** — implementation code, test code, review artifacts, work-item body content, and any file not listed in the Required Reading Order below are prohibited for the orchestrator to read. Delegate all such reading to subagents.
2. **Do not ask the user for delegation permission** — when the user request is already a task-execution command, launch required subagent roles immediately and autonomously. Treating the absence of an explicit spawn request as a reason to pause is a violation.
3. **Do not self-execute review, implementation, or judgement** — no matter how simple the change, no matter whether blocked, the orchestrator must not perform implementation, review, progress judgement, or completion judgement. The only response to a blocked state is to record the failure and stop.
4. **Do not take any action other than launching subagents after active-item selection** — once the active item is selected, the only permitted orchestrator actions are launching the required subagent roles or recording launch/use failure. Proceeding to read more files, make decisions, or execute any work yourself is a violation.

All four prohibitions above are drawn from `docs/task-governance/workflow.md` (the authoritative source). These are reproduced here so they are visible immediately upon reading this skill file.

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, subagent assignment, and the full orchestration flow.
- `docs/task-governance/implementation-review-judgement.md` governs required review roles and aggregation rules for delegated review.
- `docs/task-governance/task-completion-judgement.md` governs completion criteria and the conditions under which a work item may transition to 完了.

## Required Reading Order

1. `docs/README.md`
2. `docs/tasks/README.md`
3. `docs/tasks/tasks.md`
4. The selected area `docs/tasks/<area>/README.md`

## When To Use

Use this skill for any task-execution instruction: file creation, implementation, review, progress judgement, completion judgement, or any other execution request — including any instruction to create files or execute directives of any kind. This skill must be activated before any delegated role is assigned.

When receiving an instruction, derive the orchestration intent from the current message and, when necessary, from prior conversation history. Do not assume context that is not stated; read what is present in the conversation to determine which task items are active and which roles to launch.

This skill is the repository-agnostic entry point for orchestration. For dotfiles-repository-specific orchestration (including secrets-module layer rules and domain-specific delegation constraints), use `dotfiles-task-governance` instead. Use this skill when no repository-specific skill applies.

Actor binding: while this skill is active, the current actor is the orchestrator role only, as constrained by the governing sources above. The orchestrator must not directly edit files, must not perform implementation, and must not directly execute review judgement, progress judgement, or completion judgement. Those actions must be delegated to assigned subagent roles.

## Rules

- All detailed orchestration obligations, role-separation rules, state transitions, commit-gate conditions, and fallback handling are defined in `docs/task-governance/workflow.md`. Follow that document as the authoritative source; do not reproduce or reinterpret its rules here.
- Any task-execution request starts with active-item selection from `docs/tasks/tasks.md` before assigning any role.
- After active-item selection, the only permitted orchestrator actions are launching required roles or recording launch/use failure. Do not advance to self-execution.
- Do not ask the user for delegation permission when the user request is already a task-execution command. When a task-execution command is received, launch the required subagent roles immediately and autonomously. Delegation is mandatory — do not treat the absence of an explicit spawn request as a reason to stop.
- Do not reuse a subagent across different task items, different roles, or different cycles of the same task item.
- When a fresh agent launch fails due to limits, release completed subagents and retry with a fresh agent before considering any alternative.
- Fallback execution must be delegated to an executor other than the current orchestrator.
- Even when Codex cannot commit due to sandbox constraints, the orchestrator must not perform a commit on its behalf until all required review roles — structural review, operational-consistency review, security review, specification-conformance review, test review, documentation review, and architectural-consistency review — have each returned a passing verdict. 文書是正を含む場合は参照整合レビュー担当を追加する。変更種別による必須担当の詳細は `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」セクションに従う。The absence of commit capability in a subagent does not waive any review gate.
- A passing `cargo check` result does not substitute for a passing verdict from any required review role. Build success is a necessary precondition for compilation, not evidence of architectural, specification, security, or operational correctness.
- Do not accept a subagent's completion report as the final conclusion. After each subagent returns, verify the actual repository state (e.g., `git status`, `git diff HEAD`) before proceeding to the next task.
- Before delegating a revert task, confirm that the required shell operations (`rm`, `git restore`, etc.) are permitted under the current permission mode. If the required operations are blocked, report this to the user before proceeding.
- When receiving an instruction that contains multiple numbered items, execute them in order starting from item 1. Confirm completion of each item before advancing to the next. Do not skip ahead to a later item.
- When delegating implementation work, the subagent prompt must instruct the subagent to read `.claude/skills/implementation-execution/SKILL.md` as its first action. Pass only the skill file path and the work-item path (`docs/tasks/<area>/work-items/<item>.md`). Do not inline work-item content, violation lists, checklist items, or any other content from governing sources into the prompt — the subagent must read those sources itself via the skill's Required Reading Order.
- When delegating review work, first determine the change type (実装差分 or 文書是正・文書主成果物) from the work item, then consult the「必須レビュー担当」section of `docs/task-governance/implementation-review-judgement.md` to identify the required reviewer roles for that change type:
  - 実装差分（executable behavior を含む変更）: 構造レビュー担当、運用整合レビュー担当、セキュリティレビュー担当、仕様適合レビュー担当、テストレビュー担当、ドキュメントレビュー担当、アーキテクチャ整合レビュー担当 の7担当
  - 文書是正・文書主成果物: 運用整合レビュー担当、参照整合レビュー担当 の2担当
  - 高リスク変更を含む場合は変更種別によらずセキュリティレビュー担当を追加する
  Launch each required reviewer as a **separate independent subagent**. Do not consolidate multiple reviewer roles into a single subagent. Do not inline judgment conditions, verification procedures, output destinations, or any other content from governing sources into the prompt — each subagent must read those sources itself via its skill's Required Reading Order. Pass parameters to each reviewer as follows — do not pass parameters not listed for that role:
  - **構造レビュー担当**: 対象コードが存在するリポジトリパス（例: `rust/dotfiles-cli/src/`）のみを渡す。作業定義文書パスを渡してはならない。
  - **仕様適合レビュー担当**: 作業定義文書パス（`docs/tasks/<area>/work-items/<item>.md`）とレビュー対象コードパスの両方を渡す。
  - **セキュリティレビュー担当**: レビュー対象コードパスのみを渡す。作業定義文書パスは渡してはならない。
  - **運用整合レビュー担当**: 作業定義文書パスとレビュー対象コードパスの両方を渡す。
  - **参照整合レビュー担当**: レビュー対象文書パスのみを渡す。
  - **テストレビュー担当**: 作業定義文書パス（`docs/tasks/<area>/work-items/<item>.md`）とレビュー対象コードパスの両方を渡す。
  - **ドキュメントレビュー担当**: レビュー対象コードパスのみを渡す。作業定義文書パスは渡してはならない。
  - **アーキテクチャ整合レビュー担当**: レビュー対象モジュールのコードパス**全体**（例: `rust/dotfiles-cli/src/secrets/`）のみを渡す。差分や個別ファイルではなくモジュール全体のパスを渡すこと（全体としての設計整合を判定する役割であるため）。作業定義文書パスを渡してはならない。

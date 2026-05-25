---
name: dotfiles-task-governance
description: Use this skill when repository task flow needs orchestration, role assignment, ledger-update delegation control, or coarse-grained history recovery with minimal context.
---

# Dotfiles Task Governance

## Absolute Prohibitions (read this before anything else)

The following are unconditionally forbidden for the orchestrator. No exception exists.

1. **Do not read files outside the Required Reading Order** — implementation code, test code, review artifacts, work-item body content, and any file not listed in the Required Reading Order below are prohibited for the orchestrator to read. Delegate all such reading to subagents.
2. **Do not ask the user for delegation permission** — when the user request is already a task-execution command, launch required subagent roles immediately and autonomously. Treating the absence of an explicit spawn request as a reason to pause is a violation.
3. **Do not self-execute review, implementation, or judgement** — no matter how simple the change, no matter whether blocked, the orchestrator must not perform implementation, review, progress judgement, or completion judgement. The only response to a blocked state is to record the failure and stop.
4. **Do not take any action other than launching subagents after active-item selection** — once the active item is selected, the only permitted orchestrator actions are launching the required subagent roles or recording launch/use failure. Proceeding to read more files, make decisions, or execute any work yourself is a violation.

All four prohibitions above are drawn from `docs/task-governance/workflow.md` (the authoritative source). These are reproduced here so they are visible immediately upon reading this skill file.

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, and subagent assignment.
- `docs/task-governance/implementation-review-judgement.md` and `docs/task-governance/task-completion-judgement.md` govern delegated review/completion boundaries.

## Governed Ledger Source

- `docs/tasks/tasks.md` is the single task entrypoint and active-item selection source, and is mandatory while this skill is active.
- The active item selected in `docs/tasks/tasks.md` must directly point to the execution-governing materials for that item under `docs/tasks/<area>/...`, and those referenced materials must be followed.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/task-completion-judgement.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. The selected area `docs/tasks/<area>/README.md`
9. `docs/architecture/hexagonal-implementation-rules.md` (architecture rules — understand before delegating to implementation or review roles)
10. `docs/architecture/review-checklist.md` (per-directory check items derived from hexagonal-implementation-rules.md — must be included in all review role delegation instructions)

## When To Use

Use this skill whenever a task needs to be advanced, started, or continued — including "proceed with the task", "complete the task", "advance the current item", or any similar task-execution instruction. This skill is the entry point for all task progression.

Use this skill for orchestration-only work: selecting the active task, assigning roles for delegated ledger updates, or recovering lost progress history.

Actor binding: while this skill is active, the current actor is the orchestrator role only, as constrained by the governing sources above and the governed ledger source above. The actor must not directly edit files, must not switch into implementation, review, progress-judgement, or completion-judgement roles, and must not run implementation work or perform review/progress/completion judgement.

## Rules

- Keep context minimal; do not pre-read implementation specs or target code for delegated roles.
- Any task-execution request starts with orchestration from `docs/tasks/tasks.md` active-item selection.
- Immediately after active-item selection, the orchestrator may only launch delegated roles or record launch/use failure in the governing record.
- Before launch success/failure handling is completed, do not read target code/spec/tests/review artifacts for implementation judgement, do not run tests, and do not edit files.
- If the user request is already a task-execution command, do not ask for additional delegation permission.
- While using this skill, do not read target implementation code, implementation specs, architecture rules, or review artifacts in order to judge implementation sufficiency yourself. That work belongs to delegated implementation, review, and completion-judgement roles.
- Treat completion or progress continuation requests as requests to advance the currently selected active work item from the root active ledger (`docs/tasks/tasks.md`), not as permission to sweep branch-wide diffs and close unrelated or inactive items.
- Do not infer implementation sufficiency, review pass, or completion from coarse signals alone. For work items whose primary artifact is an executable code diff, those judgements are valid only when delegated roles trace the current code against the work-item completion conditions and violation-remediation targets, and confirm that no unresolved items remain.
- Do not perform implementation-sufficiency judgement, review judgement, or completion judgement in the orchestrator role when those roles can be assigned separately.
- The orchestrator must not execute implementation, review, progress judgement, or completion judgement directly, and must delegate those tasks to assigned roles.
- Do not reuse a subagent across different task items or different roles.
- If a fresh launch fails because of an agent/thread limit, first release completed subagents and retry with a fresh agent. Launch limits never justify reusing a subagent that is already assigned to another task item or role.
- Fallback execution must be delegated to an executor other than the current orchestrator. The current orchestrator cannot serve as the fallback executor.
- When delegating implementation work, the subagent prompt must instruct the subagent to read `.claude/skills/implementation-execution/SKILL.md` as its first action. Pass only the skill file path and the work-item path (`docs/tasks/<area>/work-items/<item>.md`). Do not inline work-item content, violation lists, checklist items, or any other content from governing sources into the prompt — the subagent must read those sources itself via the skill's Required Reading Order.
- When delegating review work, first determine the change type (実装差分 or 文書是正・文書主成果物) from the work item, then consult the「必須レビュー担当」section of `docs/task-governance/implementation-review-judgement.md` to identify the required reviewer roles for that change type:
  - 実装差分（executable behavior を含む変更）: 構造レビュー担当、運用整合レビュー担当、セキュリティレビュー担当、仕様適合レビュー担当 の4担当
  - 文書是正・文書主成果物: 運用整合レビュー担当、参照整合レビュー担当 の2担当
  - 高リスク変更を含む場合は変更種別によらずセキュリティレビュー担当を追加する
  Launch each required reviewer as a **separate independent subagent**. Pass only the corresponding reviewer skill file path (`.agents/skills/<reviewer-name>/SKILL.md`) and the work-item path to each subagent. Do not consolidate multiple reviewer roles into a single subagent. Do not inline judgment conditions, verification procedures, output destinations, or any other content from governing sources into the prompt — each subagent must read those sources itself via its skill's Required Reading Order.
- The orchestrator must never edit files directly, read target code for implementation judgement, run tests, or perform any delegated role's work — even for "simple" fixes, even when blocked, even when asked by the user. The only response to a blocked state is to record the failure and stop.
- When `secrets.rs` or any other source file has a compilation error introduced by a previous subagent, the fix must be delegated to a fresh implementation-executor subagent, not performed by the orchestrator directly.
- The orchestrator must never initiate commit-related work (S3→S4 transition) unless all required review roles (structural, specification-conformance, security, operational) have returned a recorded `合格` verdict. 文書是正を含む場合は参照整合レビュー担当を追加する。変更種別による必須担当の詳細は `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」セクションに従う。Skipping any required review role and proceeding to commit is unconditionally forbidden, regardless of how simple or low-risk the change appears.
- If all required review roles have not yet been completed, the orchestrator must launch the missing review role subagents and wait for their verdicts before allowing commit-related work. The orchestrator must not substitute its own judgement for a missing reviewer's verdict.
- When a subagent (e.g., Codex) cannot commit due to sandbox constraints, the orchestrator may perform the commit on its behalf only after all required review roles have returned recorded `合格` verdicts. The inability of a subagent to commit is never a reason to bypass review gates.
- When a subagent's implementation contains compilation errors, the orchestrator must not fix the errors directly using Edit or Write tools. The fix must be delegated to a fresh implementation-executor subagent.
- When `git restore`, `rm`, or other revert commands are blocked by permission constraints, the orchestrator must not attempt to recover by overwriting files with the Write tool. The orchestrator must report the blocked operation to the user and wait for explicit permission or alternative instruction before proceeding.
- Do not accept a subagent's completion report as the final conclusion. After each subagent returns, verify the actual repository state (e.g., `git status`, `git diff HEAD`) before proceeding to the next task.
- Before delegating a revert task, confirm that the required shell operations (`rm`, `git restore`, etc.) are permitted under the current permission mode. If the required operations are blocked, report this to the user before proceeding.
- When receiving an instruction that contains multiple numbered items, execute them in order starting from item 1. Confirm completion of each item before advancing to the next. Do not skip ahead to a later item.

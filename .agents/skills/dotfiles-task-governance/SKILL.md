---
name: dotfiles-task-governance
description: Use this skill when repository task flow needs orchestration, role assignment, ledger-update delegation control, or coarse-grained history recovery with minimal context.
---

# Dotfiles Task Governance

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
- When delegating implementation work for secret-recovery items, the orchestrator must confirm that the implementation executor's governing sources include `docs/architecture/hexagonal-implementation-rules.md` and the secrets-module layer mapping defined therein.
- When delegating review work, the orchestrator must confirm that the reviewer's governing sources include the layer-based constraint rules (not just file-name-specific rules) from `docs/architecture/hexagonal-implementation-rules.md`.
- The orchestrator must never edit files directly, read target code for implementation judgement, run tests, or perform any delegated role's work — even for "simple" fixes, even when blocked, even when asked by the user. The only response to a blocked state is to record the failure and stop.
- When `secrets.rs` or any other source file has a compilation error introduced by a previous subagent, the fix must be delegated to a fresh implementation-executor subagent, not performed by the orchestrator directly.
- The orchestrator must never initiate commit-related work (S3→S4 transition) unless all required review roles (structural, specification-conformance, security, operational) have returned a recorded `合格` verdict. Skipping any required review role and proceeding to commit is unconditionally forbidden, regardless of how simple or low-risk the change appears.
- If all required review roles have not yet been completed, the orchestrator must launch the missing review role subagents and wait for their verdicts before allowing commit-related work. The orchestrator must not substitute its own judgement for a missing reviewer's verdict.
- When a subagent (e.g., Codex) cannot commit due to sandbox constraints, the orchestrator may perform the commit on its behalf only after all required review roles have returned recorded `合格` verdicts. The inability of a subagent to commit is never a reason to bypass review gates.
- When a subagent's implementation contains compilation errors, the orchestrator must not fix the errors directly using Edit or Write tools. The fix must be delegated to a fresh implementation-executor subagent.
- When `git restore`, `rm`, or other revert commands are blocked by permission constraints, the orchestrator must not attempt to recover by overwriting files with the Write tool. The orchestrator must report the blocked operation to the user and wait for explicit permission or alternative instruction before proceeding.

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

## When To Use

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

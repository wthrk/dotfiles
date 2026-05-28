---
name: task-completion-judgement
description: Use this skill when a subagent must decide whether a work item can transition to completed based on completion criteria and required evidence.
---

# Task Completion Judgement

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, and subagent assignment.
- `docs/task-governance/implementation-review-judgement.md` governs required review roles and aggregated review-pass dependency for completion decisions.
- `docs/task-governance/task-completion-judgement.md` and `docs/task-governance/progress-judgement.md` govern completion criteria and evidence requirements.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/task-completion-judgement.md`
6. `docs/task-governance/progress-judgement.md`
7. `docs/task-governance/security-obligations.md`
8. `docs/tasks/README.md`
9. `docs/tasks/tasks.md`
10. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)
11. Relevant confirmation/review artifacts under `docs/tasks/<area>/review-artifacts/`

`docs/tasks/<area>/tasks.md` is mandatory only when the active work item explicitly references it.

## When To Use

Use this skill for `completed` transition decisions after confirmation/review stages.

Actor binding: while this skill is active, the current actor is the task-completion-judgement role under the governing sources above.

## Rules

- Judge completion only; do not re-run review-start or aggregation judgement.
- Require evidence completeness from the same change set before approving completion.
- Select the active item from `docs/tasks/tasks.md`, verify the completion target aligns with that item, and follow that item's referenced task definitions and confirmation/review artifacts as execution-governing sources.

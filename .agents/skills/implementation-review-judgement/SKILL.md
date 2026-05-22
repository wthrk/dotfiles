---
name: implementation-review-judgement
description: Use this skill when a subagent must judge implementation review-start readiness and multi-reviewer aggregation.
---

# Implementation Review Judgement

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, and subagent assignment.
- `docs/task-governance/implementation-review-judgement.md` governs review-start gates, reviewer roles, and aggregation rules.
- `docs/task-governance/security-obligations.md` governs security constraints that are binding for review judgement and recorded artifacts.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/security-obligations.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)
9. Relevant `docs/tasks/<area>/review-artifacts/...`

`docs/tasks/<area>/tasks.md` is mandatory only when the active work item explicitly references it.

## When To Use

Use this skill for review-start gate checks and review aggregation checks.

Actor binding: while this skill is active, the current actor is the implementation-review-judgement role under the governing sources above.

## Rules

- Judge only review-start and aggregation conditions.
- Delegate completion judgement to `task-completion-judgement`.
- Select the active item from `docs/tasks/tasks.md`, verify the review target aligns with that item, and follow that item's referenced review artifacts and area task definitions as execution-governing sources.

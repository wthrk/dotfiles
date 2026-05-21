---
name: task-completion-judgement
description: Use this skill when a subagent must decide whether a work item can transition to 完了 based on completion criteria and required evidence.
---

# Task Completion Judgement

## When To Use

Use this skill for `完了` transition decisions after confirmation/review stages.

## Required Reading

1. `docs/README.md`
2. `docs/task-governance/task-completion-judgement.md`
3. `docs/task-governance/progress-judgement.md`
4. `docs/task-governance/security-obligations.md`
5. `docs/tasks/<area>/tasks.md`
6. Relevant confirmation/review artifacts under `docs/tasks/<area>/review-artifacts/`

## Rules

- Judge completion only; do not re-run review-start or aggregation judgement.
- Require evidence completeness from the same change set before approving completion.
- Follow subagent assignment constraints from `docs/task-governance/workflow.md`.

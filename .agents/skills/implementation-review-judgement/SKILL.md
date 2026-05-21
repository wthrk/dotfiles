---
name: implementation-review-judgement
description: Use this skill when a subagent must judge implementation review-start readiness and multi-reviewer aggregation.
---

# Implementation Review Judgement

## When To Use

Use this skill for review-start gate checks and review aggregation checks.

## Required Reading

1. `docs/README.md`
2. `docs/task-governance/implementation-review-judgement.md`
3. `docs/task-governance/security-obligations.md`
4. `docs/tasks/<area>/tasks.md`
5. Relevant `docs/tasks/<area>/review-artifacts/...`

## Rules

- Judge only review-start and aggregation conditions.
- Delegate completion judgement to `task-completion-judgement`.
- Follow subagent assignment constraints from `docs/task-governance/workflow.md`.

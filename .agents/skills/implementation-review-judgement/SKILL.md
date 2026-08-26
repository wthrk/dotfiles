---
name: implementation-review-judgement
description: Use this skill when a subagent must judge review-start readiness and aggregate required reviewer verdicts.
---

# Implementation Review Judgement

## Actor Binding

While this skill is active, the current actor is the **review aggregation judge**.

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. The user-specified GitHub issue, PR, explicit task, or delegated review input
5. Additional canonical documents required by the input

## Rules

- Perform only the review aggregation judge's judgement; do not edit source files, commit, act as an individual reviewer, or perform another role's work.
- Do not execute tests, builds, or verification commands; judge strictly through direct static inspection of the target code, documents, issue, PR, or task.
- Read the target code, documents, issue, PR, or task directly. Do not substitute past records, summaries, or implementer reports for judgement.
- Apply the governing source for this role and avoid restating its detailed rules here.
- Return the aggregate judgement format required by `docs/task-governance/implementation-review-judgement.md`.

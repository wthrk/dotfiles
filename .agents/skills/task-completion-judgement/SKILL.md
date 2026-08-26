---
name: task-completion-judgement
description: Use this skill when deciding whether a work unit can be completed.
---

# Task Completion Judgement

## Actor Binding

While this skill is active, the current actor is the **task-completion judge**.

## Governing Sources

- `docs/task-governance/task-completion-judgement.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/task-completion-judgement.md`
4. The user-specified GitHub issue, PR, explicit task, or delegated review input
5. Additional canonical documents required by the input

## Rules

- Perform only the task-completion judge's judgement; do not edit source files, commit, act as a reviewer, or perform another role's work.
- Do not execute tests, builds, or verification commands; judge strictly through direct static inspection of the target diff, review verdicts, and verification records.
- Read the target code, documents, issue, PR, or task directly. Do not substitute past records, summaries, or implementer reports for judgement.
- Apply the governing source for this role and avoid restating its detailed rules here.
- Return the completion judgement required by `docs/task-governance/task-completion-judgement.md`.

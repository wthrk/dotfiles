---
name: structural-review
description: Use this skill when acting as the structural reviewer.
---

# Structural Review

## Actor Binding

While this skill is active, the current actor is the **structural reviewer**.

## Governing Sources

- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/task-governance/implementation-review-judgement.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/architecture/hexagonal-implementation-rules.md`
5. The user-specified GitHub issue, PR, explicit task, or delegated review input
6. Additional canonical documents required by the input

## Rules

- Perform only this role's judgement; do not edit source files, commit, or perform another role's work.
- Read the target code, documents, issue, PR, or task directly. Do not substitute past records, summaries, or implementer reports for judgement.
- Apply the governing source for this role and avoid restating its detailed rules here.
- Return the verdict format required by `docs/task-governance/implementation-review-judgement.md` when acting as a reviewer.

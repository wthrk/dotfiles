---
name: test-review
description: Use this skill when acting as the test reviewer.
---

# Test Review

## Actor Binding

While this skill is active, the current actor is the **test reviewer**.

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`
- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/architecture/review-checklist.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/architecture/hexagonal-implementation-rules.md`
5. `docs/architecture/review-checklist.md`
6. The user-specified GitHub issue, PR, explicit task, or delegated review input
7. Additional canonical documents required by the input

## Rules

- Perform only this role's judgement; do not edit source files, commit, or perform another role's work.
- Read the target code, documents, issue, PR, or task directly. Do not substitute past records, summaries, or implementer reports for judgement.
- Apply the architecture test-placement rules when judging test doubles, fixtures, inline unit tests, internal backend stubs, and test-only observation boundaries.
- Apply the governing source for this role and avoid restating its detailed rules here.
- Return the verdict format required by `docs/task-governance/implementation-review-judgement.md` when acting as a reviewer.

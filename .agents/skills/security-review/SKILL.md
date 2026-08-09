---
name: security-review
description: Use this skill when acting as the security reviewer.
---

# Security Review

## Actor Binding

While this skill is active, the current actor is the **security reviewer**.

## Governing Sources

- `docs/task-governance/security-obligations.md`
- `docs/task-governance/implementation-review-judgement.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/task-governance/security-obligations.md`
5. The user-specified GitHub issue, PR, explicit task, or delegated review input
6. Additional canonical documents required by the input

## Rules

- Perform only this role's judgement; do not edit source files, commit, or perform another role's work.
- Read the target code, documents, issue, PR, or task directly. Do not substitute past records, summaries, or implementer reports for judgement.
- Before raising a finding that asks for a new or extended check, confirm it is not a check form prohibited by `docs/docs-governance.md`; if it is, do not raise it, and require removal instead when such a check already exists.
- Apply the governing source for this role and avoid restating its detailed rules here.
- Return the verdict format required by `docs/task-governance/implementation-review-judgement.md` when acting as a reviewer.

---
name: external-dependency-conformance-review
description: Use this skill when acting as the external-dependency-conformance reviewer.
---

# External Dependency Conformance Review

## Actor Binding

While this skill is active, the current actor is the **external-dependency-conformance reviewer**.

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`
- The dependency's context7 documentation or current official documentation required by the delegated input

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. The user-specified GitHub issue, PR, explicit task, or delegated review input
5. The dependency's context7 documentation or current official documentation required by the input
6. Additional canonical documents required by the input

## Rules

- Perform only this role's judgement; do not edit source files, commit, or perform another role's work.
- Read the target code, documents, issue, PR, or task directly. Do not substitute past records, summaries, or implementer reports for judgement.
- Identify the important external SDK / crate covered by the delegated input and read its current context7 documentation when available; otherwise read the current official documentation.
- Judge conformance against documented API, behavior, and constraints rather than inference from prior knowledge alone.
- Return the verdict format required by `docs/task-governance/implementation-review-judgement.md` when acting as a reviewer.

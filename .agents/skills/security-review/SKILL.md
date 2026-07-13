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
- Apply the governing source for this role and avoid restating its detailed rules here.
- Return the verdict format required by `docs/task-governance/implementation-review-judgement.md` when acting as a reviewer.

## Test-only secret observation finding

When a review comment treats stdout, a sentinel, or state observation of a test-only dummy/fixture secret as a production secret leak, do not finalize it as a normal security finding. Request a fresh `test-secret-observation false-positive verifier` role through the review flow. The verifier must directly check compile-time test-only selection, exclusion from production build/runtime, dummy values limited to fixtures/specs, and absence of a production-reachable path. If all four checks pass, reply to the reviewer with the evidence and reject the finding; do not use it as `要修正` or `不合格`. If any check fails, continue it as a normal security finding. This exception never changes the production secret-preservation obligations.

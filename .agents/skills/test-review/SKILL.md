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
- `docs/docs-governance.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/architecture/hexagonal-implementation-rules.md`
5. `docs/architecture/review-checklist.md`
6. `docs/docs-governance.md`
7. The user-specified GitHub issue, PR, explicit task, or delegated review input
8. Additional canonical documents required by the input

## Rules

- Perform only this role's judgement; do not edit source files, commit, or perform another role's work.
- Read the target code, documents, issue, PR, or task directly. Do not substitute past records, summaries, or implementer reports for judgement.
- Before reviewing any SDK, API, or external-flow use, read the delegated task and canonical area specification/basic design/runbook first. Reconstruct purpose, storage targets, every generate/save/read/use/dispose transition, user input/output, failure/cleanup, and prohibitions; then read vendor/SDK primary sources as implementation evidence. Compare every reconstructed transition with code, tests, and doc comments. SDK material never replaces product design; stop the verdict for a required design decision when they conflict or a transition is unspecified.
- For secret-recovery, reject a recovery or `verify-yubikey --all` flow that requires anything beyond inserting the YubiKey and running the command: master password, session, PIV PIN, secret environment/argv, YubiKey OTP, or other interactive input. Require YubiKey-stored BWS credentials only, no credential output to stdout/stderr/logs/temp/persistent environment, and disposal after use.
- When the review target contains a URL, quotation, API symbol, specification section, source location, or a claim based on one, open and read the cited original material yourself. Check the claim, surrounding scope, and applicable version/revision; link or symbol existence and an implementer summary are not evidence. Apply `docs/docs-governance.md` when the source is a repository document, external specification, or SDK/crate material; state unread or unavailable sources and do not rely on them for the verdict.
- Apply the architecture test-placement rules when judging test doubles, fixtures, inline unit tests, internal backend stubs, and test-only observation boundaries.
- Apply the governing source for this role and avoid restating its detailed rules here.
- Return the verdict format required by `docs/task-governance/implementation-review-judgement.md` when acting as a reviewer.

If a finding concerns a test-only dummy/fixture secret printed through stdout, a sentinel, or state observation, route it through the fresh `test-secret-observation false-positive verifier` defined in [implementation-review-judgement.md](../../../docs/task-governance/implementation-review-judgement.md) before classifying it as a leak. Do not introduce redaction, masking, secrecy helpers, or assertions whose only purpose is to hide test input; those are prohibited by [security-obligations.md](../../../docs/task-governance/security-obligations.md). A verifier-confirmed production-reachable path remains a normal security finding.

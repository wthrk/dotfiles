---
name: structural-review
description: Use this skill when acting as the structural reviewer.
---

# Structural Review

## Actor Binding

While this skill is active, the current actor is the **structural reviewer**.

## Governing Sources

- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/architecture/review-checklist.md`
- `docs/task-governance/implementation-review-judgement.md`
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
- Apply the governing source for this role and avoid restating its detailed rules here.
- Return the verdict format required by `docs/task-governance/implementation-review-judgement.md` when acting as a reviewer.

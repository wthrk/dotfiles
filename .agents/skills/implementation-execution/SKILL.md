---
name: implementation-execution
description: Use this skill when a subagent is assigned implementation work and must produce diffs under repository execution obligations.
---

# Implementation Execution

## Actor Binding

While this skill is active, the current actor is the implementation executor.

## Governing Sources

- `docs/task-governance/implementation-execution.md`
- `docs/task-governance/security-obligations.md`
- `docs/docs-governance.md`
- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/architecture/review-checklist.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-execution.md`
4. `docs/task-governance/security-obligations.md`
5. `docs/docs-governance.md`
6. `docs/architecture/hexagonal-implementation-rules.md`
7. `docs/architecture/review-checklist.md`
8. The delegated GitHub issue, PR, explicit task, or handoff
9. The canonical specification, basic design, and runbook for the delegated area; reconstruct purpose, storage targets, each value's generate/save/read/use/dispose lifecycle, state transitions, user input/output, and prohibitions before researching SDKs
10. Only after step 9, the vendor / SDK overall-flow documentation, specifications, official samples, API documentation, and versioned source needed to realize that product flow
11. Architecture documents when code structure is in scope

## Rules

- Treat orchestration as already completed for the delegated task.
- Do not invoke `/orchestration`, and do not use `$dotfiles-task-governance` to perform orchestration, change roles, or re-select the work unit for the same delegated task.
- Do not re-select the work unit and do not launch subagents for the same implementation assignment.
- Read target files, direct dependencies, callers/callees, tests, and any handoff findings before editing.
- Compare every reconstructed product-flow transition (generate, save, read, use, dispose, input, output, failure, cleanup) against code, tests, and doc comments. SDK material is implementation evidence, never a replacement for product design; stop for a design decision if the two conflict or the product transition is unspecified.
- For secret-recovery work, enforce the canonical user contract: "the user inserts a YubiKey and runs the recovery command only." Recovery and `verify-yubikey --all` may use YubiKey-stored BWS credentials internally and must not require master password, session, PIV PIN, secret environment/argv, YubiKey OTP, or other interactive input; none of those values may reach stdout, stderr, logs, temporary files, or persistent environment.
- Produce the assigned diff, run the selected verification, and report target diff, commands, results, skipped checks, and residual risk.
- Detailed implementation duties are owned by `docs/task-governance/implementation-execution.md`.

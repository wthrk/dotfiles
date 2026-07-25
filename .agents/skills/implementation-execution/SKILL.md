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
- At start, locate and directly read every line of the approved immutable implementation handoff/design artifact required by `docs/task-governance/implementation-execution.md#不変-implementation-handoff-と-design-artifact`. Report its stable path or identifier, approval, and the canonical source/diff identity with capture time before editing; if any is absent, changes, or the handoff is incomplete or mutable, do not edit repository-authored artifacts, start review, commit, push, or run heavy whole-repository checks.
- Before editing, satisfy `docs/task-governance/implementation-execution.md#実装開始条件` by applying every required reviewer checklist to the planned design and constructing the counterexamples required by `docs/architecture/review-checklist.md#着手前設計照合`. Report the applied checklists, counterexample verdicts, and unresolved items. Do not use post-implementation review as a substitute for this design check.
- Before changing repository-authored code, directly read `docs/architecture/hexagonal-implementation-rules.md` and `docs/architecture/review-checklist.md` in full. Classify each changed responsibility as domain rule, application orchestration, port contract, adapter forwarding, support technical backend, presentation interaction, or composition wiring; verify its dependency direction, input-port boundary, adapter forwarding rule, and composition boundary before choosing a placement. Do not treat an earlier agent's summary or a passing build as a substitute for that direct reading.
- Compare every reconstructed product-flow transition (generate, save, read, use, dispose, input, output, failure, cleanup) against code, tests, and doc comments. SDK material is implementation evidence, never a replacement for product design; stop for a design decision if the two conflict or the product transition is unspecified.
- For secret-recovery work, enforce the canonical user contract: "the user inserts a YubiKey and runs the recovery command only." Recovery and `verify-yubikey --all` may use YubiKey-stored BWS credentials internally and must not require master password, session, PIV PIN, secret environment/argv, YubiKey OTP, or other interactive input; none of those values may reach stdout, stderr, logs, temporary files, or persistent environment.
- Before requesting review, commit, push, or completion, finish the handoff coverage table for every required flow, caller, state mutation, test/direct observation, document evidence, counterexample, and finding; for document-primary work include document flows, roles, reference paths, required evidence, and explicit exclusions. Recheck the canonical comparison identity at S1 completion; if it changed, discard all S1 evidence and return to a new handoff/S1. Report the completed coverage table with the assigned diff, commands, results, skipped checks, and residual risk. Do not run heavy whole-repository checks while implementation or self-reconciliation is incomplete.
- For an architectural change, report the directly read architecture sections and the resulting placement/dependency/input-port/adapter-forwarding judgement.  Keep this report tied to the implementation handoff; do not duplicate architecture rules into repository documentation.
- Detailed implementation duties are owned by `docs/task-governance/implementation-execution.md`.

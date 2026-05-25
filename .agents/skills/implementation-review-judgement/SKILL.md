---
name: implementation-review-judgement
description: Use this skill when a subagent must judge implementation review-start readiness and multi-reviewer aggregation.
---

> **Start here**: Read this entire file before taking any action. This file is your governing source — do not proceed until you have read every section.

# Implementation Review Judgement

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, and subagent assignment.
- `docs/task-governance/implementation-review-judgement.md` governs review-start gates, reviewer roles, and aggregation rules.
- `docs/task-governance/security-obligations.md` governs security constraints that are binding for review judgement and recorded artifacts.
- `docs/architecture/hexagonal-implementation-rules.md` governs layer-based architectural constraints that must be applied during structural review.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/security-obligations.md`
6. `docs/architecture/hexagonal-implementation-rules.md`
7. `docs/tasks/README.md`
8. `docs/tasks/tasks.md`
9. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)
10. Relevant `docs/tasks/<area>/review-artifacts/...`

`docs/tasks/<area>/tasks.md` is mandatory only when the active work item explicitly references it.

## When To Use

Use this skill for review-start gate checks and review aggregation checks.

Actor binding: while this skill is active, the current actor is the implementation-review-judgement role under the governing sources above.

## Rules

- Judge only review-start and aggregation conditions.
- Delegate completion judgement to `task-completion-judgement`.
- Select the active item from `docs/tasks/tasks.md`, verify the review target aligns with that item, and follow that item's referenced review artifacts and area task definitions as execution-governing sources.
- Return reviewer verdicts using the single explicit label set only: `合格`, `要修正`, `不合格`.
- Start every reviewer response with this exact structure:
  - `判定: <合格|要修正|不合格>`
  - `判定要約: <所見なし|主要論点要約>`
  - `根拠:`
- Do not use freeform lead verdicts such as `通しません`, `No findings`, `指摘なし`, `no blockers`, or `pass` in place of the explicit `判定` line.
- When the verdict is `合格`, use `判定要約: 所見なし`.
- When any concern, residual risk, unresolved doubt, follow-up item, or operational dependency remains, the verdict must be at least `要修正`; record the remediation condition in `根拠:` and do not emit `合格`.
- Structural review must apply layer-based rules from `docs/architecture/hexagonal-implementation-rules.md`, not only file-name-specific violation targets. If an `adapters/` file exposes any item — whether `pub`, `pub(crate)`, or `pub(super)` — that is not a port trait implementation, emit `判定: 不合格`. Helper functions (stdin readers, prompt functions, JSON decoders, terminal I/O functions) are not port trait implementations regardless of where they are defined.
- When reviewing secret-recovery code diffs: for each modified file, identify its layer from the secrets module layer mapping in `docs/architecture/hexagonal-implementation-rules.md`, then verify that the file's contents satisfy that layer's responsibility constraints and prohibition rules.
- A diff that passes compilation but violates layer-based constraints must receive `判定: 不合格` regardless of test results.
- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, must not commit changes, and must not perform any implementation work — even to fix a defect found during review. All remediation must be delegated back to the implementation executor.
- `cargo check` passing, tests passing, and build success are not substitutes for a review verdict. A review subagent must trace the actual code against the work-item completion conditions and violation-remediation targets before returning `合格`.
- **Review independence**: The reviewer MUST read and inspect the actual code directly. Past review records, confirmation records, or implementer reports must NOT substitute for independent judgment. Even if previous cycle records show a pass, the reviewer must personally verify the current code before returning a pass verdict. The existence of prior review records is never grounds to skip direct code inspection.
- For each modified file in the diff, identify its layer from the directory-to-layer mapping in `docs/architecture/hexagonal-implementation-rules.md`, then open `docs/architecture/review-checklist.md` and verify every check item in the section corresponding to that layer. A violation of any check item in the applicable section must result in `判定: 不合格`. Do not duplicate checklist content here — the canonical source is `docs/architecture/review-checklist.md`.
- When reviewing `adapters/` files: do not rely on the work-item's listed "対象コードパス" as the complete set of files to inspect. Open the `adapters/` directory directly and enumerate every file present. For each file, list every `pub`, `pub(crate)`, and `pub(super)` symbol. Any symbol that is not a port trait implementation type or its method must result in `判定: 不合格`. This check is not satisfied by confirming that the previously-known violation list (e.g., V12/V13) has been resolved — the reviewer must independently enumerate all current public symbols.
- When reviewing `adapters/` files: the `adapters.rs` (or `adapters/mod.rs`) file must be inspected to determine whether any child module is re-exported as `pub(super)` or higher. If a child module is `pub(super)`, its exported symbols become accessible from the parent module. Trace this export chain and apply the port-trait-implementation-only rule to all symbols reachable from outside `adapters/`.
- Philosophical review obligation: For each layer encountered in the diff, ask the philosophical question for that layer before applying checklist items. The questions are defined in `docs/architecture/review-checklist.md` under "レビュー時の問い" for each layer section. A file that passes all checklist items but fails a philosophical question must receive `判定: 不合格`. Checklist compliance is necessary but not sufficient for a passing verdict. For the specific questions to ask per layer, refer to the "レビュー時の問い" section in `docs/architecture/review-checklist.md` — do not rely on any inline restatement here.

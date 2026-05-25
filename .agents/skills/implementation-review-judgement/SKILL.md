---
name: implementation-review-judgement
description: Use this skill when a subagent must judge implementation review-start readiness and multi-reviewer aggregation.
---

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

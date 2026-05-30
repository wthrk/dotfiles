---
name: structural-review
description: Use this skill when a subagent is assigned as the structural reviewer to review implementation diffs for layer-based architectural compliance, dependency direction, and visibility rules.
---

# Structural Review

## Role

**Structural Reviewer**

Apply layer-specific responsibility, dependency-direction, and visibility rules from `docs/architecture/hexagonal-implementation-rules.md`. A primary obligation is to apply layer "review questions" from `docs/architecture/review-checklist.md` before checklist matching. If any answer indicates "violates layer philosophy", return `Verdict: Fail` even if checklist items pass. Explicitly record answers to layer-philosophy questions in `Rationale:`. Even when file placement or naming looks correct, return `Verdict: Fail` if implemented responsibilities violate layer philosophy.

Do not accept mechanical separation as architectural compliance. Classify meaningful processing units using the responsibility boundaries defined in `docs/architecture/hexagonal-implementation-rules.md` and verify that each unit is placed in its prescribed boundary. Thin ports or adapters must not be preserved by moving responsibilities into layers that the canonical architecture assigns elsewhere.

## Input Parameters

**Review target code path only** (example: `rust/dotfiles-cli/src/`).

A work-definition document path and task list are not provided. Do not read them on your own. Task-specific violation IDs (V12/V13 etc.) are not input for this role; checklist matching based on those IDs is prohibited.

## Governing Sources

- `docs/architecture/hexagonal-implementation-rules.md` governs layer responsibility, dependency direction, and visibility.
- `docs/architecture/review-checklist.md` governs philosophical questions and per-directory check items to apply.
- `docs/task-governance/implementation-review-judgement.md` governs verdict format and aggregation rules.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/architecture/hexagonal-implementation-rules.md`
4. `docs/architecture/review-checklist.md`
5. `docs/task-governance/implementation-review-judgement.md`

## Rules

Layer-based rules from `docs/architecture/hexagonal-implementation-rules.md` take precedence over file-name-specific rules.

### Step 1 - Philosophical Validation (mandatory, before checklist matching)

- Open `docs/architecture/review-checklist.md` and read "review questions" for all layers in scope.
- Read code and answer each question explicitly in the form "this code does only ...".
- Before accepting placement, classify meaningful processing units by the canonical responsibility boundaries and record why the chosen layer is the prescribed boundary, not merely where the code was moved.
- For `support`, distinguish responsibilities assigned to support from responsibilities assigned to other layers by the canonical architecture documents.
- For Bitwarden Secrets Manager paths, apply the responsibility allocation defined by the canonical architecture and secret-recovery design documents; do not restate those detailed rules here.
- Record each answer in `Rationale:`.
- If even one answer is "violates philosophy", immediately fix verdict to `Verdict: Fail` and do not proceed to Step 2.
- Do not proceed to Step 2 without completing Step 1.
- A review that does not record philosophical-question answers in `Rationale:` is incomplete and must not be submitted.

### Step 2 - Checklist Matching (only when Step 1 has no philosophy violation)

- Enumerate all files under `adapters/` and all `pub`/`pub(crate)`/`pub(super)` symbols.
  - If any symbol is not a port-trait implementation, return `Verdict: Fail`.
  - Identify modules re-exported as `pub(super)` or wider from `adapters.rs` (or `adapters/mod.rs`) and apply the same rule to all externally reachable symbols.
- For adapter-local `#[cfg(feature = "secrets-internal-test-stub")]` backend stubs, apply the internal backend stub conditions in `docs/architecture/hexagonal-implementation-rules.md` before treating them as production-source test double mixing. If all conditions are met, do not fail solely because the adapter-local stub exists. This does not relax adapter visibility: unnecessary `pub(super)` helpers still fail.
- Verify there is no adapter concrete-type import and no `println!`/stdin read in `application/` files.
- For private helpers in `application/` and `adapters/`, describe each helper's responsibility in one sentence and judge whether it belongs to that layer. If any helper is unexplained or layer-mismatched, return `Verdict: Fail`.
- If helpers are heavily split, evaluate port-capability granularity (coarse contract) as a potential cause, not only code organization. If root cause is port design, return `Verdict: Fail` and require port re-splitting even when files are already split.
- Apply other corresponding layer check items in `docs/architecture/review-checklist.md`. Any violation returns `Verdict: Fail`. Do not duplicate checklist content here.

### Common Constraints

- Reviewer scope is verdict only. Do not edit source files, commit changes, or perform implementation work.
- **Review independence**: Read and inspect actual code directly. Past review records, confirmation records, and implementer reports are not substitutes.
- **Re-review scope**: Even in re-review after rework, do not carry over the previous session. Each review is a fresh session and must re-verify all items.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate verdict-format rules here. Record philosophical-question answers explicitly in `Rationale:`.

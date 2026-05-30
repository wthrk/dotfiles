---
name: test-review
description: Use this skill when a subagent is assigned as the test reviewer to verify that tests cover the work item's completion conditions and that test doubles/fixtures are not mixed into the production source tree.
---

# Test Review

## Role

**Test Reviewer**

Verify that test code actually validates the specification (completion conditions in the work-definition document). Also verify, using responsibility-based judgement rather than form alone, that test doubles (definitions of Fake/Stub/Mock) and fixtures are not mixed into the production source tree.

## Input Parameters

**Both** the work-definition document path (`docs/tasks/<area>/work-items/<item>.md`) and the review target code path.

## Governing Sources

- `docs/tasks/<area>/work-items/<item>.md` (the active work item's work definition document) governs the specific completion conditions that tests must cover.
- `docs/task-governance/implementation-review-judgement.md` governs verdict format and aggregation rules.
- `docs/architecture/hexagonal-implementation-rules.md` governs layer boundaries, including the rules for `tests/` layer placement.
- `docs/architecture/review-checklist.md` governs per-directory check items including `tests/` layer checks.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/architecture/hexagonal-implementation-rules.md` (to confirm placement rules for the `tests/` layer)
4. `docs/architecture/review-checklist.md` (check items for paths under `tests/`)
5. `docs/task-governance/implementation-review-judgement.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. The provided work-definition document path

## Rules

- **Coverage of completion conditions by tests**: For each item listed in `Completion Conditions` in the work-definition document, verify that tests exist to validate it. If any completion-condition item has no validating test, return `Verdict: Fail`.
- **Responsibility-based judgement**: For test-double leakage, judge by **responsibility** rather than by form (file naming, `#[cfg(test)]` vs `#[cfg(feature = "...")]`, or whether it implements a port trait). For each symbol, file, and gate block, ask "What is its responsibility?" and "Does that responsibility belong to this layer?" If the responsibility does not belong to the layer, return `Verdict: Fail` even when the form looks valid. Canonical criteria are the `tests/` section and `responsibility-based judgement principles` in `docs/architecture/review-checklist.md`; do not duplicate them here.
- **No test-double definitions in production layers**: If types that stand in for real dependencies in tests (Fake/Stub/Mock **definitions**) exist in production layers (`adapters/`, `application/`, `domain/`, `ports/`, `support/`, etc.), return `Verdict: Fail`. `#[cfg(test)]` wrapping, `#[cfg(feature = "...")]` gates, and port-trait implementations are not exemptions. Move them to the `tests/` layer or a dedicated test-support crate.
- **Internal backend stub exception**: Adapter-local backend stubs gated only by `secrets-internal-test-stub` are allowed when they satisfy the canonical internal backend stub conditions: production build exclusion, compile-time selection without runtime real/stub branching, unchanged production command path, unchanged port contract, no domain/business logic moved into the stub, integration tests executing the feature-enabled `dotfiles` binary without importing adapter stub modules, and fixture/state helpers remaining in `tests/`. If any condition is missing, return `Verdict: Fail`.
- **Inline unit tests are not prohibited**: Regular inline unit tests in production-layer `src/` files (`#[cfg(test)] mod tests { #[test] fn ... }`) are idiomatic Rust for verifying private functions in that module and are allowed. Do not return `Verdict: Fail` only because `#[test]` functions or `#[cfg(test)]` blocks exist. The prohibition is limited to double **definitions** placed in production layers. The decision boundary is responsibility, not form (self-module verification vs stand-in definitions for real dependencies).
- **Direct code confirmation**: Open actual files to verify test existence and placement. Do not substitute summaries or implementer reports.
- **Review independence**: Past review records, confirmation records, and implementer reports must not substitute for judgement. The reviewer must read target code directly and judge independently.
- **Reviewer scope of responsibility**: The reviewer only returns a verdict. Do not directly edit source files or commit. Send all fixes back to the implementation executor.
- Verdict format follows `docs/task-governance/implementation-review-judgement.md`. Do not duplicate verdict-format rules here. Explicitly list each checked item and its result in `Rationale:`.

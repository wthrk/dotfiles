---
name: documentation-review
description: Use when acting as the documentation reviewer for code doc comments.
---

# Documentation Review

## Actor Binding

While this skill is active, the current actor is the **documentation reviewer**.

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md` (section: documentation reviewer responsibilities)
- `docs/architecture/hexagonal-implementation-rules.md` (section: document comment rules)
- `docs/docs-governance.md` (if present)

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/architecture/hexagonal-implementation-rules.md`
5. `docs/docs-governance.md` (if present)

## Rules

- Judge alignment between implementation and code doc comments.
- Use canonical scope, including mandatory production targets and test-case exclusion defined in governing sources.
- Return verdict only; do not edit implementation.
- Do not add local normative rules here.

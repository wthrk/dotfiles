---
name: pr-mergeability-loop
description: Use as a support skill when a PR instruction involves review response, AI/Codex/Copilot review, PR-scoped @codex review, review thread resolution, checks, or mergeability confirmation.
---

# PR Mergeability Loop

## Actor Binding

This is a support skill only. It does not establish or change the current actor's role.

The actor remains bound to the role skill already invoked, such as `/implementation-execution`, a review skill, or a judgement skill. Delegated role actors must not use this skill to re-orchestrate, re-select the work unit, or launch subagents for the same delegated task.

## Governing Sources

- `docs/task-governance/pr-mergeability-loop.md`
- `docs/task-governance/workflow.md`, especially PR operation rules
- `docs/docs-governance.md` when documentation placement or canonical-source handling is in scope

## Required Reading Order

1. The role skill that established the current actor
2. `docs/task-governance/pr-mergeability-loop.md`
3. `docs/task-governance/workflow.md` PR operation rules
4. `docs/docs-governance.md` when documentation placement or canonical-source handling is in scope

## When To Use

Use this skill with the current role when the instruction includes any of the following:

- PR URL or PR number together with a mergeability, checks, review thread, or PR review response request
- PR review response
- AI review, Codex review, Copilot review, or PR-scoped `@codex review`
- review thread reply or resolve
- checks confirmation
- mergeability confirmation

## Rules

- Apply `docs/task-governance/pr-mergeability-loop.md` directly; this skill does not restate the durable loop procedure.
- Use this skill only within the actor's already established role and permissions.
- Report any operation that the current role cannot perform instead of treating this support skill as authority to perform it.

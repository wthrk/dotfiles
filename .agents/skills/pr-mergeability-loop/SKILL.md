---
name: pr-mergeability-loop
description: Use as a support skill when a PR instruction involves review response, AI/Codex/Copilot review, @codex review, review thread resolution, checks, or mergeability confirmation.
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

- PR URL or PR number
- PR review response
- AI review, Codex review, Copilot review, or `@codex review`
- review thread reply or resolve
- checks confirmation
- mergeability confirmation

## Rules

- Apply `docs/task-governance/pr-mergeability-loop.md` directly.
- Treat the goal as making the PR confirmably mergeable, not performing the merge.
- Work against the latest PR head OID and repeat after every pushed fix.
- Confirm checks, mergeability, unresolved review threads, and latest-head AI/Codex/Copilot review status before reporting completion.
- Reply to adopted or rejected review comments as required, and resolve completed threads when permitted.
- If checks are pending or failing, external review is incomplete, resolve permission is missing, conflicts exist, branch protection is unsatisfied, or latest-head AI/Codex/Copilot review cannot be obtained, report the hold condition instead of calling the loop complete.

---
name: implementation-execution
description: Use this skill when a subagent is assigned implementation work and must produce diffs under repository execution obligations.
---

# Implementation Execution

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, and subagent assignment.
- `docs/task-governance/implementation-execution.md` governs execution obligations, evidence recording, and remediation scope.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-execution.md`
5. `docs/tasks/README.md`
6. `docs/tasks/tasks.md`
7. `docs/tasks/<area>/README.md`
8. `docs/tasks/<area>/work-items/<item>.md`
9. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)
10. `docs/<area>/implementation-guidelines.md` (if present)

`docs/tasks/<area>/tasks.md` is mandatory only when the active work item explicitly references it.

## When To Use

Use this skill for implementation assignments.

Actor binding: while this skill is active, the current actor is the implementation executor role under the governing sources above.

## Rules

- Follow required references, file-category reread obligations, and recording requirements from `docs/task-governance/implementation-execution.md`.
- Select the active item from `docs/tasks/tasks.md`, then follow that item's required references under `docs/tasks/<area>/...` as execution-governing sources.
- For review-feedback remediation scope, follow the binding full-scope reviewer-perspective rule in `docs/task-governance/implementation-execution.md`.
- Apply the implementation rule text as written, including `最小構成で済まそうとしてはならない。`.
- Smallest diff and inherited-structure preservation are not goals; redesign to compliant structure (including zero-base module/document boundary rewrites when needed).

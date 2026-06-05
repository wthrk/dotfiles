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
- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/architecture/review-checklist.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-execution.md`
4. `docs/task-governance/security-obligations.md`
5. `docs/architecture/hexagonal-implementation-rules.md`
6. `docs/architecture/review-checklist.md`
7. The delegated GitHub issue, PR, explicit task, or handoff
8. Canonical area documents required by the delegated work
9. Architecture documents when code structure is in scope

## Rules

- Treat orchestration as already completed for the delegated task.
- Do not invoke `/orchestration`, and do not use `$dotfiles-task-governance` to perform orchestration, change roles, or re-select the work unit for the same delegated task.
- Do not re-select the work unit and do not launch subagents for the same implementation assignment.
- Read target files, direct dependencies, callers/callees, tests, and any handoff findings before editing.
- Produce the assigned diff, run the selected verification, and report target diff, commands, results, skipped checks, and residual risk.
- Detailed implementation duties are owned by `docs/task-governance/implementation-execution.md`.

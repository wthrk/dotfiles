---
name: orchestration
description: Use when the main agent receives a top-level task-execution request and must select the work unit and delegate roles under repository governance.
---

# Orchestration

## Actor Binding

While this skill is active, the current actor is the orchestrator.

## Governing Sources

- `docs/task-governance/workflow.md`
- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/security-obligations.md`
- `docs/docs-governance.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/security-obligations.md`
6. `docs/docs-governance.md`
7. The user-specified GitHub issue, PR, or explicit task
8. Follow `docs/README.md` to the target area's README and its canonical documents, including documents not explicitly named in the task. For package additions, reintroductions, or updates, read `README.md` (「更新と適用」) and follow `docs/automation/README.md` to the nightly lock-bump and Homebrew cask-pinning documents before delegation.

## Rules

- Select exactly one work unit from the user-specified GitHub issue, PR, or explicit task.
- Extract only the delegation parameters needed to launch roles.
- Before delegation, establish the existing update and activation path from those canonical sources and include the requested scope in the handoff: repository declaration changes, delivery through commit / PR / main, or machine activation. A package reintroduction request in this repository is a declaration change unless the user requests machine activation; do not delegate a direct install in its place.
- Launch required fresh role agents; do not self-execute implementation, review, completion judgement, tests, builds, or file edits.
- Do not poll running subagents or read their transcript/logs during execution; wait for event-driven completion/message notifications to preserve context isolation.
- Do not ask for extra delegation permission when the user request is already a task-execution command.
- Record launch/use failure only when a required role cannot be launched.
- Detailed prohibitions and branch/PR gates are owned by `docs/task-governance/workflow.md`.

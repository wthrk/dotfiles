---
name: orchestration
description: Use this skill whenever a task-execution instruction is received — including file creation, implementation, review, or completion judgement — to orchestrate work via active-item selection and subagent delegation.
---

# Orchestration

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, subagent assignment, and the full orchestration flow.
- `docs/task-governance/implementation-review-judgement.md` governs required review roles and aggregation rules for delegated review.
- `docs/task-governance/task-completion-judgement.md` governs completion criteria and the conditions under which a work item may transition to 完了.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/task-completion-judgement.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. The selected area `docs/tasks/<area>/README.md`
9. `docs/architecture/hexagonal-implementation-rules.md` (when delegating implementation or review work touching `rust/` — understand layer boundaries before assigning roles)

## When To Use

Use this skill for any task-execution instruction: file creation, implementation, review, progress judgement, completion judgement, or any other execution request. This skill must be activated before any delegated role is assigned.

This skill is the repository-agnostic entry point for orchestration. For dotfiles-repository-specific orchestration (including secrets-module layer rules and domain-specific delegation constraints), use `dotfiles-task-governance` instead. Use this skill when no repository-specific skill applies.

Actor binding: while this skill is active, the current actor is the orchestrator role only, as constrained by the governing sources above. The orchestrator must not directly edit files, must not perform implementation, and must not directly execute review judgement, progress judgement, or completion judgement. Those actions must be delegated to assigned subagent roles.

## Rules

- All detailed orchestration obligations, role-separation rules, state transitions, commit-gate conditions, and fallback handling are defined in `docs/task-governance/workflow.md`. Follow that document as the authoritative source; do not reproduce or reinterpret its rules here.
- Any task-execution request starts with active-item selection from `docs/tasks/tasks.md` before assigning any role.
- After active-item selection, the only permitted orchestrator actions are launching required roles or recording launch/use failure. Do not advance to self-execution.
- Do not ask the user for delegation permission when the user request is already a task-execution command. When a task-execution command is received, launch the required subagent roles immediately and autonomously. Delegation is mandatory — do not treat the absence of an explicit spawn request as a reason to stop.
- Do not reuse a subagent across different task items, different roles, or different cycles of the same task item.
- When a fresh agent launch fails due to limits, release completed subagents and retry with a fresh agent before considering any alternative.
- Fallback execution must be delegated to an executor other than the current orchestrator.

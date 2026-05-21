---
name: dotfiles-task-governance
description: Use this skill when repository task flow needs orchestration, role assignment, task-ledger repair, or coarse-grained history recovery with minimal context.
---

# Dotfiles Task Governance

## When To Use

Use this skill for orchestration-only work: selecting the active task, assigning roles, repairing ledgers, or recovering lost progress history.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/tasks/README.md`
4. The selected area `docs/tasks/<area>/README.md`
5. The selected area `docs/tasks/<area>/tasks.md`

## Rules

- Keep context minimal; do not pre-read implementation specs or target code for delegated roles.
- Follow `docs/task-governance/workflow.md` for role assignment, fallback, and subagent assignment rules.
- Do not reuse a subagent across different task items or different roles.

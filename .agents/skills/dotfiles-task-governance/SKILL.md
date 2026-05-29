---
name: dotfiles-task-governance
description: Use with orchestration when repository-specific dotfiles or secret-recovery governance constraints must be applied from canonical documents.
---

# Dotfiles Task Governance

## Canonical Sources

- `docs/task-governance/workflow.md`
- `docs/task-governance/implementation-execution.md`
- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/task-completion-judgement.md`
- `docs/task-governance/security-obligations.md`
- `docs/secret-recovery/implementation-guidelines.md`
- `docs/docs-governance.md`

## Required Reading Order

Use this order as navigation pointers only; role skills and canonical documents own the detailed rules.

1. The role skill that established the current actor's role
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. The task-governance document for the current role or gate
5. `docs/task-governance/security-obligations.md`
6. `docs/secret-recovery/implementation-guidelines.md` when secret-recovery is in scope
7. `docs/docs-governance.md` when documentation placement or canonical-source handling is in scope

## When To Use

Use this skill only as repository-specific governance support for an actor whose role has already been established by the applicable role skill.

This skill does not replace `/orchestration`, `/implementation-execution`, review skills, or judgement skills. Delegated role actors must not use this skill to switch themselves into an orchestrator role for the same delegated task.

## Rule

Repository-specific task flow, role separation, review gates, completion gates, evidence handling, security obligations, and secret-recovery constraints are owned by the canonical documents listed above. This skill intentionally does not restate or reinterpret those detailed rules.

Before acting in a governed area, read the canonical source for that area and apply it directly. If this skill conflicts with a canonical governance document, stop and follow the canonical document rather than this summary.

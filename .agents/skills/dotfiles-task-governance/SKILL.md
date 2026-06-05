---
name: dotfiles-task-governance
description: Use with an already established role when repository-specific dotfiles or secret-recovery governance constraints must be applied from canonical documents.
---

# Dotfiles Task Governance

## Actor Binding

This skill does not establish or change the current actor's role. The actor remains bound to the role skill already invoked.

## Governing Sources

- `docs/task-governance/security-obligations.md`
- `docs/secret-recovery/implementation-guidelines.md`
- `docs/docs-governance.md`

## Required Reading Order

1. The role skill that established the current actor
2. `docs/task-governance/README.md`
3. `docs/task-governance/security-obligations.md`
4. `docs/secret-recovery/implementation-guidelines.md`
5. `docs/docs-governance.md`
6. The canonical document for the governed area

## Rules

- Use this only as repository-specific governance support.
- Do not use this skill to switch a delegated role actor into orchestrator for the same delegated task.
- Apply canonical documents directly; this skill intentionally does not restate their detailed rules.

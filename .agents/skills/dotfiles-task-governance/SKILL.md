---
name: dotfiles-task-governance
description: Use with an already established role when repository-specific dotfiles or secret-recovery governance constraints must be applied from canonical documents.
---

# Dotfiles Task Governance

## Actor Binding

This skill does not establish or change the current actor's role. The actor remains bound to the role skill already invoked.

## Governing Sources

- `docs/task-governance/security-obligations.md`
- `docs/task-governance/pr-mergeability-loop.md`
- `docs/secret-recovery/implementation-guidelines.md`
- `docs/docs-governance.md`

## Required Reading Order

1. The role skill that established the current actor
2. `docs/task-governance/README.md`
3. `docs/task-governance/security-obligations.md`
4. `docs/task-governance/pr-mergeability-loop.md` when a PR URL or PR number is specified, or when PR review response, AI/Codex/Copilot review, `@codex review`, review thread resolve, checks confirmation, or mergeability confirmation is in scope
5. `docs/secret-recovery/implementation-guidelines.md`
6. `docs/docs-governance.md`
7. The canonical document for the governed area

## Rules

- Use this only as repository-specific governance support.
- Do not use this skill to switch a delegated role actor into orchestrator for the same delegated task.
- Also use `/pr-mergeability-loop` when PR instructions include a PR URL or PR number, PR review response, AI/Codex/Copilot review, `@codex review`, review thread resolve, checks confirmation, or mergeability confirmation.
- Apply canonical documents directly; this skill intentionally does not restate their detailed rules.

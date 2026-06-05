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
4. `docs/task-governance/pr-mergeability-loop.md` when the main orchestrator is handling a PR URL or PR number together with a mergeability-related operation, PR review response, PR-scoped AI/Codex/Copilot review, PR-scoped `@codex review`, review thread resolve, PR-scoped checks confirmation, PR-scoped mergeability confirmation, or another explicitly PR mergeability-related request
5. `docs/secret-recovery/implementation-guidelines.md`
6. `docs/docs-governance.md`
7. The canonical document for the governed area

## Rules

- Use this only as repository-specific governance support.
- Do not use this skill to switch a delegated role actor into orchestrator for the same delegated task.
- For top-level PR instructions that include a PR URL or PR number together with a mergeability-related operation, PR review response, PR-scoped AI/Codex/Copilot review, PR-scoped `@codex review`, review thread resolve, PR-scoped checks confirmation, PR-scoped mergeability confirmation, or another explicitly PR mergeability-related request, the main orchestrator also uses `/pr-mergeability-loop` as an orchestration extension.
- Delegated actors do not use `/pr-mergeability-loop` to take over the full PR loop. They follow their assigned role and report PR-loop-relevant facts back to the parent orchestrator.
- Apply canonical documents directly; this skill intentionally does not restate their detailed rules.

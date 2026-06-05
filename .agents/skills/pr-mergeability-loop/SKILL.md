---
name: pr-mergeability-loop
description: Use as an orchestration extension for the main orchestrator when a PR instruction involves review response, AI/Codex/Copilot review, PR-scoped @codex review, review thread resolution, PR-scoped checks, or PR-scoped mergeability confirmation.
---

# PR Mergeability Loop

## Actor Binding

This is an orchestration-extension support skill for the main orchestrator. It does not establish or change the current actor's role and does not create a standalone delegated PR-loop role.

For top-level PR mergeability requests, the main orchestrator uses this skill with `/orchestration` to coordinate the loop. Delegated implementation, review, judgement, and commit / PR operation actors must not use this skill to take over the full loop, re-orchestrate, re-select the work unit, or launch subagents for the same delegated task. They stay bound to their assigned role and report PR-loop-relevant facts back to the parent orchestrator.

## Governing Sources

- `docs/task-governance/pr-mergeability-loop.md`
- `docs/task-governance/workflow.md`, especially `## 8. ブランチ・コミット・プルリクエスト運用`
- `docs/docs-governance.md` when documentation placement or canonical-source handling is in scope

## Required Reading Order

1. The role skill that established the current actor
2. `docs/task-governance/pr-mergeability-loop.md`
3. `docs/task-governance/workflow.md` `## 8. ブランチ・コミット・プルリクエスト運用`
4. `docs/docs-governance.md` when documentation placement or canonical-source handling is in scope

## When To Use

The main orchestrator uses this skill as an orchestration extension when the top-level instruction includes any of the following:

- PR URL or PR number accompanied by a PR mergeability-related operation or request, such as PR-scoped checks, PR-scoped mergeability confirmation, review thread handling, PR review response, AI/Codex/Copilot review response, or thread resolve
- PR review response
- PR-scoped AI review, Codex review, Copilot review, or `@codex review`
- review thread reply or resolve
- PR-scoped checks confirmation
- PR-scoped mergeability confirmation

## Rules

- Apply `docs/task-governance/pr-mergeability-loop.md` directly; this skill does not restate the durable loop procedure.
- This skill augments `/orchestration` for PR mergeability requests; it does not replace `/orchestration`, `/implementation-execution`, review skills, judgement skills, or the commit / PR operation rules in `workflow.md`.
- The main orchestrator coordinates target PR selection, PR state inventory, bounded delegation, result aggregation, and repeated re-checks until completion or a blocked condition is known.
- Delegated actors use their assigned role skill for their bounded task and report checks, review thread, review result, commit / push, or PR operation facts back to the parent orchestrator. They do not own the full AI / PR loop.

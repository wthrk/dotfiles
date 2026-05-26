---
name: implementation-execution
description: Use this skill when a subagent is assigned implementation work and must produce diffs under repository execution obligations.
---

# Implementation Execution

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, and subagent assignment.
- `docs/task-governance/implementation-execution.md` governs execution obligations, evidence recording, and remediation scope.
- `docs/architecture/hexagonal-implementation-rules.md` governs layer-based responsibility boundaries, allowed and forbidden artifacts per layer, and visibility rules.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-execution.md`
5. `docs/architecture/hexagonal-implementation-rules.md`
6. `docs/architecture/review-checklist.md` (per-directory check items — apply these constraints during implementation, not only during review)
7. `docs/tasks/README.md`
8. `docs/tasks/tasks.md`
9. `docs/tasks/<area>/README.md`
10. `docs/tasks/<area>/work-items/<item>.md`
11. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)
12. `docs/<area>/implementation-guidelines.md` (if present)

`docs/tasks/<area>/tasks.md` is mandatory only when the active work item explicitly references it.

## When To Use

Use this skill for implementation assignments.

Actor binding: while this skill is active, the current actor is the implementation executor role under the governing sources above.
If this skill was provided by a parent orchestrator for a delegated task, treat orchestration as already completed for that task. Do not re-enter `/orchestration`, do not re-select the active item, and do not spawn subworkers for the same delegated implementation assignment.

## Rules

- Read orchestrator-only instructions in `AGENTS.md` and `docs/task-governance/workflow.md` as constraints on the parent orchestrator unless the current request itself is a new task-execution request addressed to you as the top-level agent.
- For a delegated implementation assignment, execute the implementation directly after the Required Reading Order. Do not reply that "the main agent is always orchestrator" and do not convert the delegated assignment into a new orchestration cycle.
- Follow required references, file-category reread obligations, and recording requirements from `docs/task-governance/implementation-execution.md`.
- Read `docs/tasks/tasks.md` as part of the required repository context, but when a parent orchestrator has already supplied the work-item path for a delegated task, treat that supplied work item as the already-selected active item. Do not use `docs/tasks/tasks.md` to re-select a different item or start a new orchestration cycle.
- For review-feedback remediation scope, follow the binding full-scope reviewer-perspective rule in `docs/task-governance/implementation-execution.md`.
- Apply the implementation rule text as written, including `最小構成で済まそうとしてはならない。`.
- Smallest diff and inherited-structure preservation are not goals; redesign to compliant structure (including zero-base module/document boundary rewrites when needed).
- Before writing any code in `adapters/`, verify that the implementation exposes only port trait implementations. Any item that is not a port trait implementation — whether declared `pub`, `pub(crate)`, or `pub(super)` — is a layer violation and must be removed or made private. Helper functions such as stdin readers, prompt functions, JSON decoders, and terminal I/O functions are not port trait implementations and must be `fn` (private) inside the adapter file, even if they were `pub(crate)` before.
- Before writing any code in `application/`, verify that no adapter concrete types are imported and no `println!` / stdin reads are present.
- Layer-based rules from `docs/architecture/hexagonal-implementation-rules.md` take precedence over file-name-specific rules. When a file-name-specific violation target (e.g., V1〜V16 in `yubikey.md`) appears resolved but a layer-based violation persists, the item is NOT resolved.
- Before finalizing any change, identify the layer of each modified directory from the directory-to-layer mapping in `docs/architecture/hexagonal-implementation-rules.md`, then open `docs/architecture/review-checklist.md` and apply every check item in the section that corresponds to that layer. If any check item is violated, stop and resolve the violation before proceeding. Do not duplicate checklist content here — the canonical source is `docs/architecture/review-checklist.md`.
- Before writing any code, identify the layer of each directory to be changed using the directory-to-layer mapping in `docs/architecture/hexagonal-implementation-rules.md`, then read the "レビュー時の問い" (philosophical questions) for that layer in `docs/architecture/review-checklist.md` and answer each question for the planned implementation. If any answer is "this implementation violates the philosophy of the layer", modify the implementation even if all checklist items would pass. An implementation that cannot answer the philosophical questions is incomplete and must not be submitted.

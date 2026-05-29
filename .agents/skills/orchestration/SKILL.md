---
name: orchestration
description: Use when the main agent receives a top-level task-execution request and must select the active item and delegate roles under repository governance.
---

# Orchestration

## Canonical Sources

- `docs/README.md`
- `docs/tasks/README.md`
- `docs/tasks/tasks.md`
- `docs/task-governance/workflow.md`
- `docs/docs-governance.md`

## Required Reading Order

Use this order as navigation pointers only; the canonical documents own the detailed rules.

1. `docs/README.md`
2. `docs/tasks/README.md`
3. `docs/tasks/tasks.md`
4. `docs/task-governance/README.md`
5. `docs/task-governance/workflow.md`
6. Active work item references required by `docs/task-governance/workflow.md`
7. `docs/docs-governance.md` when documentation placement or canonical-source handling is in scope

## When To Use

Use this skill only while acting as the main-agent orchestrator for a top-level task-execution request in this repository.

Delegated implementation, review, progress-judgement, and completion-judgement actors do not become the orchestrator for the delegated task. They use their assigned role skill and execute the delegated role directly.

## Rule

`docs/task-governance/workflow.md` is the canonical source for task flow, active-item selection, allowed orchestrator actions, role separation, delegation obligations, transport-agnostic role handling, and failure handling. This skill intentionally does not restate or reinterpret those detailed rules.

Before any orchestrator action, follow the entry sequence and active-item/delegation flow defined by `docs/task-governance/workflow.md`, using `docs/README.md`, `docs/tasks/README.md`, and `docs/tasks/tasks.md` as the repository entry points.

If this skill conflicts with a canonical governance document, stop and follow the canonical document rather than this summary.

---
name: implementation-review-judgement
description: Use this skill when a subagent must judge implementation review-start readiness and multi-reviewer aggregation.
---

> **Start here**: Read this entire file before taking any action. This file is your governing source — do not proceed until you have read every section.

# Implementation Review Judgement (Aggregation Role)

This skill is an **aggregation role**. Its only responsibility is to receive reviewer verdicts returned by each reviewer skill and emit an aggregated verdict. Individual reviewer verdicts must be performed by independent subagents using corresponding reviewer skills. The aggregation role must not perform those reviewer judgements itself.

## Reviewer Skill File List

The delegated reviewer roles and their skill-file paths are as follows.

- **Structural Reviewer**: `.agents/skills/structural-review/SKILL.md`
- **Specification-Conformance Reviewer**: `.agents/skills/specification-conformance-review/SKILL.md`
- **Security Reviewer**: `.agents/skills/security-review/SKILL.md`
- **Operational-Consistency Reviewer**: `.agents/skills/operational-consistency-review/SKILL.md`
- **Test Reviewer**: `.agents/skills/test-review/SKILL.md`
- **Documentation Reviewer**: `.agents/skills/documentation-review/SKILL.md`
- **Architectural-Consistency Reviewer**: `.agents/skills/architectural-consistency-review/SKILL.md`
- **Reference-Integrity Reviewer**: `.agents/skills/reference-integrity-review/SKILL.md`

## Governing Sources

- `docs/task-governance/workflow.md` governs role assignment, fallback, and subagent assignment.
- `docs/task-governance/implementation-review-judgement.md` governs review-start gates, reviewer roles, required reviewer assignment per change type, verdict format, and aggregation rules. This document is the canonical source — do not duplicate its rules here.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/tasks/README.md`
6. `docs/tasks/tasks.md`
7. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)
8. Relevant `docs/tasks/<area>/review-artifacts/...`

`docs/tasks/<area>/tasks.md` is mandatory only when the active work item explicitly references it.

## When To Use

Use this skill for review-start gate checks and review aggregation checks.

Actor binding: while this skill is active, the current actor is the implementation-review-judgement aggregation role under the governing sources above.

## Rules

- The aggregation role's sole responsibility is to receive verdicts from each required reviewer and issue the aggregate verdict. The aggregation role must not perform individual reviewer duties itself.
- Determine which reviewer roles are required for the change type by reading the `必須レビュー担当` section in `docs/task-governance/implementation-review-judgement.md`. Each required reviewer must be launched as a separate fresh subagent with its corresponding skill file path individually specified.
- Judge only review-start and aggregation conditions. Delegate completion judgement to `task-completion-judgement`.
- Aggregation verdict format, label set, and rules are governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate those rules here.
- When any required reviewer returns a verdict, record that verdict before proceeding to aggregation.
- If any required reviewer cannot be launched, record that fact and the handling as specified by `docs/task-governance/implementation-review-judgement.md`.

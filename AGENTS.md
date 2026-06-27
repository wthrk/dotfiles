# AGENTS.md

This is the minimal session-entry document for this repository. It keeps only language policy, skill-first entry order, role-to-skill binding, orchestrator prohibition pointers, and translation synchronization.

## Communication

- Respond in Japanese unless the user explicitly requests another language.
- Write code review findings, PR summaries, and verification notes in Japanese.
- Keep technical identifiers, commands, paths, commit types, and upstream quotations in their original form when needed.

## Skill-First Entry

- For every top-level task-execution request, invoke `/orchestration` before reading task references or doing role work.
- The first references below are read as the `/orchestration` skill's Required Reading Order, not as pre-skill work.

## First References

- Start from [docs/README.md](docs/README.md), then [docs/task-governance/README.md](docs/task-governance/README.md) and [docs/task-governance/workflow.md](docs/task-governance/workflow.md).
- Apply [docs/docs-governance.md](docs/docs-governance.md) for document placement, canonical-source handling, and duplication rules.
- Role details live in `.agents/skills/*/SKILL.md`; do not restate those rules here.

## Role-to-Skill Binding

Every role must invoke its designated skill before doing role work.

| Role | Skill |
|---|---|
| Orchestrator | `/orchestration` |
| Repository-specific governance support | `/dotfiles-task-governance` |
| Implementation executor | `/implementation-execution` |
| Review aggregation | `/implementation-review-judgement` |
| Completion judgement | `/task-completion-judgement` |

Individual reviewer roles use these skill files:

| Reviewer role | Skill file |
|---|---|
| Structural reviewer | `.agents/skills/structural-review/SKILL.md` |
| Operational-consistency reviewer | `.agents/skills/operational-consistency-review/SKILL.md` |
| Security reviewer | `.agents/skills/security-review/SKILL.md` |
| Specification-conformance reviewer | `.agents/skills/specification-conformance-review/SKILL.md` |
| External-dependency-conformance reviewer | `.agents/skills/external-dependency-conformance-review/SKILL.md` |
| Test reviewer | `.agents/skills/test-review/SKILL.md` |
| Documentation reviewer | `.agents/skills/documentation-review/SKILL.md` |
| Architectural-consistency reviewer | `.agents/skills/architectural-consistency-review/SKILL.md` |
| Reference-integrity reviewer | `.agents/skills/reference-integrity-review/SKILL.md` |

Delegated role agents do not become the main agent for the delegated task. A delegated implementation executor starts with `/implementation-execution`, does not invoke `/orchestration` for the same delegated task, and does not launch further subagents for that delegated implementation assignment.

## Orchestrator Prohibitions

The orchestrator's absolute prohibitions and permitted actions are defined in [docs/task-governance/workflow.md](docs/task-governance/workflow.md). This file intentionally does not duplicate them.

## Translation Synchronization

- `AGENTS_ja.md` must remain semantically aligned with `AGENTS.md`.
- If `AGENTS.md` is edited, update `AGENTS_ja.md` in the same change.
- If a new `AGENTS.md` is added anywhere in this repository, add the sibling `AGENTS_ja.md` in the same change.

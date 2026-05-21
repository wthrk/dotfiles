# AGENTS.md

## Scope

This file applies to everything under `.agents/skills/`.

## Translation Synchronization

- `AGENTS_ja.md` must remain semantically aligned with `AGENTS.md`.
- If `AGENTS.md` is edited, update `AGENTS_ja.md` in the same change.
- During review, verify semantic equivalence between both documents.

## Important Rules

- Keep skills thin. Put durable rules in `docs/`, not in `SKILL.md`.
- Do not duplicate normative prose from `docs/` into skills.
- When a skill changes its references, verify the referenced documents and headings still exist.
- When a skill changes content that other files rely on, verify the incoming references (`被参照`) still make sense.

## Required References

- Before editing any skill, you must read `docs/README.md`.
- When the skill refers to repository workflow or task rules, you must read `docs/task-governance/README.md`.
- When the skill refers to area task artifacts, you must read `docs/tasks/README.md`.

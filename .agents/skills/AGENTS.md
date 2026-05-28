# AGENTS.md

## Scope

This file applies to everything under `.agents/skills/`.

## Translation Synchronization

- `AGENTS_ja.md` must remain semantically aligned with `AGENTS.md`.
- If `AGENTS.md` is edited, update `AGENTS_ja.md` in the same change.
- During review, verify semantic equivalence between both documents.

## Important Rules

- Every repository-authored `SKILL.md` must explicitly bind the current actor/role in unambiguous prose.
- Every repository-authored `SKILL.md` must use one primary prose language per file. Do not mix English and Japanese prose in the same file.
- Allowed exceptions to the single-language rule are file paths, identifiers, required upstream terms, and exact quoted rule text.
- If a repository-authored `SKILL.md` depends on external governing documents, declare them in a top-level governing-sources/required-reading section before any local interpretation or trailing rules.
- When a skill changes its references, verify the referenced documents and headings still exist.
- When a skill changes content that other files rely on, verify the incoming references (`被参照`) still make sense.

## Skill File Authoring Rules

- Every `SKILL.md` (including its front matter and body) must be written in English.
- Detailed instructions and durable normative content must be recorded in an appropriate file under `docs/`. A `SKILL.md` must reference that file rather than reproduce the content inline.
- Whenever a `SKILL.md` is created or modified, a Japanese translation `SKILL_ja.md` must be created or updated in the same change as an accurate Japanese rendering of the English source.

## Required References

- Before editing any skill, you must read `docs/README.md`.
- When the skill refers to repository workflow or task rules, you must read `docs/task-governance/README.md`.
- When the skill refers to area task artifacts, you must read `docs/tasks/README.md`.

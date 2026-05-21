# AGENTS.md

## Scope

This file applies to everything under `docs/`.

## Translation Synchronization

- `AGENTS_ja.md` must remain semantically aligned with `AGENTS.md`.
- If `AGENTS.md` is edited, update `AGENTS_ja.md` in the same change.
- During review, verify semantic equivalence between both documents.

## Important Rules

- Keep `docs/` as the source of truth for durable repository rules.
- Do not duplicate normative prose between documents.
- Split documents by responsibility instead of growing mixed-purpose files.
- Keep README files as entrypoints and navigation only.
- When changing any document that is referenced elsewhere, verify the incoming references (`被参照`) still point to the right file and the right heading.

## Required References

- Before editing under `docs/`, you must read [docs-governance.md](./docs-governance.md).
- When the change touches task rules or task structure, you must read [task-governance/README.md](./task-governance/README.md).
- When the change touches area task artifacts, you must read [tasks/README.md](./tasks/README.md).

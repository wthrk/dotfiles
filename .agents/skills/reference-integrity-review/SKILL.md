---
name: reference-integrity-review
description: Use this skill when a subagent is assigned as the reference-integrity reviewer (document remediation only) to verify that links, references, file paths, and definitions within documents are consistent and resolvable.
---

# Reference Integrity Review

## Role

**Reference-Integrity Reviewer (Document Remediation Only)**

Verify consistency of links, reference targets, file paths, and definitions within documents. If reference targets do not exist or definitions and references are inconsistent, return `Verdict: Fail`.

## Input Parameters

**Review target document path only**. A work-definition document path is not provided.

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/workflow.md`
- `docs/docs-governance.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/tasks/README.md`
6. `docs/tasks/tasks.md`
7. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)
8. Relevant `docs/tasks/<area>/review-artifacts/...`

## Rules

- This role applies to document remediation and document-primary deliverable reviews only. It is not a required reviewer for implementation diffs unless those diffs also include document changes.
- For each document in the review target, verify: all links and file path references resolve to existing files; all cross-references and defined terms are used consistently; no definition appears in one place and contradicts its usage elsewhere.
- If any reference target does not exist, or if a definition and its usage are inconsistent, emit `Verdict: Fail` and list the specific broken references or inconsistencies in `Rationale:`.
- Verify conformance to `docs/docs-governance.md` for target documents, including canonical-source and duplication rules.
- Treat document-only supplemental-record references according to `docs/task-governance/workflow.md`; do not add stricter self-hash, exact file-set, ledger-sync, or current-cycle wording requirements in this skill.
- If the target is a skill file (`SKILL.md`), additionally verify frontmatter, required-reading coverage, and canonical-source duplication rules.
- Do not apply exact tracked-file counts, exact file-set enumeration, ledger synchronization, confirmation/review artifact synchronization, or current-cycle wording equality as a review gate condition.
- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, commit changes, or perform implementation work.
- **Review independence**: Read and inspect the actual documents directly. Past review records, confirmation records, or implementer reports must not substitute for independent judgment. Even if previous cycle records show a pass, personally verify before returning a pass verdict.
- **Re-review scope**: Even when re-reviewing after rework, do not carry over the previous review session. Each review must be conducted as an independent new session. Previously passed items must not be skipped; apply the target scope defined by the governing sources.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate the verdict format rules here — the canonical source is that document.

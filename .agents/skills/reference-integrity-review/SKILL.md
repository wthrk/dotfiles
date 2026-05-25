---
name: reference-integrity-review
description: Use this skill when a subagent is assigned as the 参照整合レビュー担当 (document remediation only) to verify that links, references, file paths, and definitions within documents are consistent and resolvable.
---

# Reference Integrity Review

## 役割

**参照整合レビュー担当（文書是正専用）**

文書内のリンク・参照先・ファイルパス・定義の一貫性を確認する。参照先が存在しない・定義と参照が不一致の場合は `判定: 不合格` とする。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md` governs verdict format, aggregation rules, and the applicable review targets (document remediation and document-primary deliverables).
- `docs/task-governance/workflow.md` governs role assignment and subagent obligations.

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
- If any reference target does not exist, or if a definition and its usage are inconsistent, emit `判定: 不合格` and list the specific broken references or inconsistencies in `根拠:`.
- If `docs/docs-governance.md` exists, verify that each target document conforms to the document conventions defined there.
- If the target is a skill file (`SKILL.md`), additionally verify: (1) the frontmatter contains both `name` and `description` fields and their content is consistent with what the skill actually does; (2) a `Required Reading Order` section exists and all references necessary for skill execution are listed without omission; (3) the file conforms to the prohibition on duplicating canonical-source content (governing source content must not be reproduced inside the skill file).
- Do not apply exact tracked-file counts or exact file-set enumeration as a review gate condition. The minimum record is: what changes were reviewed, what references were checked, and what verdict was returned.
- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, must not commit changes, and must not perform any implementation work. All remediation must be delegated back to the implementation executor.
- **Review independence**: Read and inspect the actual documents directly. Past review records, confirmation records, or implementer reports must not substitute for independent judgment. Even if previous cycle records show a pass, personally verify before returning a pass verdict.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate the verdict format rules here — the canonical source is that document.

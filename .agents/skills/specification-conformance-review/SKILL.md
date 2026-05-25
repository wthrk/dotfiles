---
name: specification-conformance-review
description: Use this skill when a subagent is assigned as the 仕様適合レビュー担当 to verify that implementation diffs satisfy the work item's specific completion conditions, violation remediation targets, and structural completion criteria.
---

# Specification Conformance Review

## 役割

**仕様適合レビュー担当**

作業定義文書の `規約違反の解消対象`・`構造完了条件`・`完了条件` を現行コードに対して直接照合する。各項目について現行コードを開いて未解消が残っていないかを確認する。サマリーや実装担当の報告で代替してはならない。

## Governing Sources

- `docs/tasks/<area>/work-items/<item>.md` (the active work item's work definition document) governs the specific review perspectives, constraints, completion conditions, and violation remediation targets for this review.
- `docs/task-governance/implementation-review-judgement.md` governs verdict format and aggregation rules.
- `docs/task-governance/workflow.md` governs role assignment and subagent obligations.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/tasks/README.md`
6. `docs/tasks/tasks.md`
7. `docs/tasks/<area>/work-items/<item>.md` — **mandatory**: read this document yourself; do not rely on summaries or implementer reports
8. Area-specific artifacts referenced by the active work item (`docs/tasks/<area>/...`)
9. Relevant `docs/tasks/<area>/review-artifacts/...`

## Rules

- **Mandatory direct reading**: Read the active work item's work definition document (`docs/tasks/<area>/work-items/<item>.md`) yourself. Do not rely on summaries, implementer reports, or prior review records to determine what the work item requires.
- **Direct code inspection**: For each item listed in `規約違反の解消対象`, `構造完了条件`, and `完了条件` in the work definition document, open the relevant source files and verify directly against the current code that the condition is satisfied. Do not infer satisfaction from build success, test results, or prior cycle records.
- Every specific constraint, completion condition, and violation remediation target stated in the work definition document must be individually checked. If any item cannot be confirmed as resolved in the current code, emit `判定: 不合格` or `判定: 要修正` and list the unresolved items in `根拠:`.
- `cargo check` passing, tests passing, and build success are not substitutes for this review. The reviewer must trace actual code against each work-item completion condition before returning `合格`.
- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, must not commit changes, and must not perform any implementation work. All remediation must be delegated back to the implementation executor.
- **Review independence**: Read and inspect the actual code directly. Past review records, confirmation records, or implementer reports must not substitute for independent judgment. Even if previous cycle records show a pass, personally verify the current code before returning a pass verdict.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate the verdict format rules here — the canonical source is that document. List each checked condition and its result explicitly in `根拠:`.

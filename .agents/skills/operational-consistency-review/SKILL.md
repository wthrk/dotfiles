---
name: operational-consistency-review
description: Use this skill when a subagent is assigned as the 運用整合レビュー担当 to verify that execution procedures, role separation, gate conditions, audit trail requirements, and completion-judgement logic are enforceable and auditable in practice.
---

# Operational Consistency Review

## 役割

**運用整合レビュー担当**

実行手順・役割分離・ゲート条件・証跡要件・完了判定ロジックの実運用での強制可能性・監査可能性を確認する。強制可能性・監査可能性に具体的懸念がある状態で `合格` としてはならない。

## Governing Sources

- `docs/task-governance/workflow.md` governs role separation, gate conditions, and execution obligations that must be enforced in practice.
- `docs/task-governance/implementation-review-judgement.md` governs verdict format, aggregation rules, and the prohibition on downgrading concerns.

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

## Rules

- Verify that execution procedures, role-separation requirements, gate conditions, audit trail requirements, and completion-judgement logic defined in governance documents are enforced by structure — not merely by convention or documentation.
- If enforceability or auditability of any required condition is concretely in doubt, emit at minimum `判定: 要修正` with the specific concern and remediation condition stated in `根拠:`. Do not emit `合格` while such doubt remains.
- Do not downgrade concerns to "スコープ外" or "運用徹底" — if a concern exists, it must be recorded and the verdiction must reflect it.
- When any concern, residual risk, unresolved doubt, follow-up item, or operational dependency is recorded, the verdict must be at least `要修正`. Do not record concerns while simultaneously emitting `合格`.
- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, must not commit changes, and must not perform any implementation work. All remediation must be delegated back to the implementation executor.
- **Review independence**: Read and inspect the actual code and governance artifacts directly. Past review records, confirmation records, or implementer reports must not substitute for independent judgment. Even if previous cycle records show a pass, personally verify before returning a pass verdict.
- **Re-review scope**: Even when re-reviewing after a rework (差し戻し後の再実施), do not carry over the previous review session. Each review must be conducted as an independent new session. Previously passed items must not be skipped — re-verify all items. Reviewing only the rework items while omitting others is prohibited; because rework changes may have cascading effects elsewhere, the review scope must be applied to the entire codebase.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate the verdict format rules here — the canonical source is that document.

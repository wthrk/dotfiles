---
name: security-review
description: Use this skill when a subagent is assigned as the セキュリティレビュー担当 to review implementation diffs for secret exposure, unauthorized access paths, and privilege escalation risks.
---

# Security Review

## 役割

**セキュリティレビュー担当**

`docs/task-governance/security-obligations.md` に定義された制約を適用する。機密情報の露出・不正アクセス経路・権限昇格の可能性を確認する。

## Governing Sources

- `docs/task-governance/security-obligations.md` governs the security constraints binding for this review.
- `docs/task-governance/implementation-review-judgement.md` governs verdict format and aggregation rules.
- `docs/task-governance/workflow.md` governs role assignment and subagent obligations.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/security-obligations.md`
5. `docs/task-governance/implementation-review-judgement.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)
9. Relevant `docs/tasks/<area>/review-artifacts/...`

## Rules

- Apply every constraint defined in `docs/task-governance/security-obligations.md` to the review target. Do not reproduce those constraints here — the canonical source is that document.
- For each modified file, verify: absence of committed secret material (credentials, keys, session tokens), absence of secret values in log output / stdout / command args / temp files, and that failure behavior does not expose secret material.
- If the diff touches secret-recovery or secret-storage code: for each modified file, identify its layer from the secrets module layer mapping in `docs/architecture/hexagonal-implementation-rules.md`, then verify that the file's contents satisfy that layer's responsibility constraints and prohibition rules.
- When a security concern is found, record the concern, its remediation condition, and the dismissal prohibition (do not downgrade to "スコープ外" or "運用徹底") in `根拠:`.
- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, must not commit changes, and must not perform any implementation work. All remediation must be delegated back to the implementation executor.
- **Review independence**: Read and inspect the actual code directly. Past review records, confirmation records, or implementer reports must not substitute for independent judgment. Even if previous cycle records show a pass, personally verify the current code before returning a pass verdict.
- **Re-review scope**: Even when re-reviewing after a rework (差し戻し後の再実施), do not carry over the previous review session. Each review must be conducted as an independent new session. Previously passed items must not be skipped — re-verify all items. Reviewing only the rework items while omitting others is prohibited; because rework changes may have cascading effects elsewhere, the review scope must be applied to the entire codebase.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate the verdict format rules here — the canonical source is that document.

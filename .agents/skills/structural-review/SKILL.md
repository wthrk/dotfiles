---
name: structural-review
description: Use this skill when a subagent is assigned as the 構造レビュー担当 to review implementation diffs for layer-based architectural compliance, dependency direction, and visibility rules.
---

# Structural Review

## 役割

**構造レビュー担当**

`docs/architecture/hexagonal-implementation-rules.md` の層別責務・依存方向・公開範囲規則を適用する。主たる義務は `docs/architecture/review-checklist.md` の「レビュー時の問い」を各層についてコードを読む前に適用することである。問いへの答えが「哲学に違反している」であれば、チェックリスト項目が通過していても `判定: 不合格` とする。層別哲学的問いへの回答を `根拠:` に明示すること。見た目の構造（ファイル配置・命名）が正しくても実装の責務が層の哲学に反している場合は `判定: 不合格` とする。

## Governing Sources

- `docs/architecture/hexagonal-implementation-rules.md` governs layer-based responsibility, dependency direction, and visibility rules.
- `docs/architecture/review-checklist.md` governs the philosophical questions and per-directory check items that must be applied.
- `docs/task-governance/implementation-review-judgement.md` governs verdict format and aggregation rules.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/architecture/hexagonal-implementation-rules.md`
4. `docs/architecture/review-checklist.md`
5. `docs/task-governance/implementation-review-judgement.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. Area-specific artifacts required by the active work item (`docs/tasks/<area>/...`)

## Rules

- **Before reading any code**, open `docs/architecture/review-checklist.md` and read the "レビュー時の問い" section for every layer present in the review target. Then read the code and answer each question explicitly. A reviewer who cannot answer the philosophical questions for the applicable layers has not completed the review.
- If any philosophical question is answered with "this code violates the philosophical intent" — regardless of whether individual checklist items pass — emit `判定: 不合格`. Checklist compliance is necessary but not sufficient. The philosophical questions take precedence over checklist results.
- Layer-based rules from `docs/architecture/hexagonal-implementation-rules.md` take precedence over file-name-specific rules. A violation of layer philosophy overrides apparent structural correctness (correct file placement, correct naming).
- For each modified file in the diff, identify its layer from the directory-to-layer mapping in `docs/architecture/hexagonal-implementation-rules.md`, then apply every check item in the corresponding section of `docs/architecture/review-checklist.md`. A violation of any check item in the applicable section must result in `判定: 不合格`. Do not duplicate checklist content here — the canonical source is `docs/architecture/review-checklist.md`.
- When reviewing `adapters/` files: do not rely on the work item's listed "対象コードパス" as the complete set of files to inspect. Open the `adapters/` directory directly and enumerate every file present. For each file, list every `pub`, `pub(crate)`, and `pub(super)` symbol. Any symbol that is not a port trait implementation type or its method must result in `判定: 不合格`.
- When reviewing `adapters/` files: inspect `adapters.rs` (or `adapters/mod.rs`) to determine whether any child module is re-exported as `pub(super)` or higher. Trace this export chain and apply the port-trait-implementation-only rule to all symbols reachable from outside `adapters/`.
- When reviewing `application/` files: verify that no adapter concrete types are imported and no `println!` / stdin reads are present.
- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, must not commit changes, and must not perform any implementation work. All remediation must be delegated back to the implementation executor.
- **Review independence**: Read and inspect the actual code directly. Past review records, confirmation records, or implementer reports must not substitute for independent judgment. Even if previous cycle records show a pass, personally verify the current code before returning a pass verdict.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate the verdict format rules here — the canonical source is that document. Record philosophical question answers explicitly in `根拠:`.

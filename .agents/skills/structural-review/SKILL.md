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

## Rules

Layer-based rules from `docs/architecture/hexagonal-implementation-rules.md` take precedence over file-name-specific rules. A violation of layer philosophy overrides apparent structural correctness (correct file placement, correct naming).

### ステップ1 — 哲学的検証（コードを読む前に実施、必須・先行）

- `docs/architecture/review-checklist.md` を開き、レビュー対象の全層の「レビュー時の問い」を読む。
- コードを読み、各問いに「このコードは〜のみをしているか」という形で明示的に回答する。
- 回答を `根拠:` に必ず記録する。
- 1つでも「哲学に違反している」であれば、チェックリスト照合に進まず即座に `判定: 不合格` を確定する。
- ステップ1を完了せずステップ2に進んではならない。
- 哲学的問いへの回答が `根拠:` に記録されていないレビューは未完了であり提出してはならない。

### ステップ2 — チェックリスト照合（ステップ1で哲学違反なしと判定した場合のみ実施）

- `adapters/` 全ファイルを自力列挙し `pub`/`pub(crate)`/`pub(super)` シンボルを全列挙する。
  - Port trait 実装でないシンボルが1件でもあれば `判定: 不合格`。
  - `adapters.rs`（または `adapters/mod.rs`）で `pub(super)` 以上に再エクスポートされているモジュールを特定し、外部から到達可能な全シンボルに同規則を適用する。
- `application/` ファイルでは adapter 具体型 import と `println!`/stdin 読み取りがないことを確認する。
- その他 `docs/architecture/review-checklist.md` の対応層チェック項目を適用する。各違反は `判定: 不合格`。チェックリスト内容をここに複製しない — 正典は `docs/architecture/review-checklist.md`。

### 共通制約

- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, must not commit changes, and must not perform any implementation work. All remediation must be delegated back to the implementation executor.
- **Review independence**: Read and inspect the actual code directly. Past review records, confirmation records, or implementer reports must not substitute for independent judgment. Even if previous cycle records show a pass, personally verify the current code before returning a pass verdict.
- **Re-review scope**: Even when re-reviewing after a rework (差し戻し後の再実施), do not carry over the previous review session. Each review must be conducted as an independent new session. Previously passed items must not be skipped — re-verify all items. Reviewing only the rework items while omitting others is prohibited; because rework changes may have cascading effects elsewhere, the review scope must be applied to the entire codebase.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate the verdict format rules here — the canonical source is that document. Record philosophical question answers explicitly in `根拠:`.

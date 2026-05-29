---
name: reference-integrity-review
description: このスキルは、サブエージェントが参照整合レビュー担当（文書是正専用）として割り当てられたときに、文書内リンク・参照・パス・定義の解決可能性と整合性を検証するために使用する。
---

# Reference Integrity Review

## 役割

**参照整合レビュー担当（文書是正専用）**

文書内のリンク、参照先、ファイルパス、定義の整合性を確認する。参照先が存在しない、または定義と参照が不一致の場合は `Verdict: Fail` とする。

## 入力パラメーター

レビュー対象文書パスのみを受け取る。作業定義文書パスは渡されない。

## 正本参照

- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/workflow.md`
- `docs/docs-governance.md`

## 必須読込順

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/tasks/README.md`
6. `docs/tasks/tasks.md`
7. active work item が要求する領域固有資料（`docs/tasks/<area>/...`）
8. 関連する `docs/tasks/<area>/review-artifacts/...`

## ルール

- この役割は文書是正・文書主成果物レビュー専用。実装差分のみのレビューでは必須担当ではない（文書変更を含む場合は別）。
- レビュー対象文書ごとに、リンク/パス参照の解決可能性、定義・用語・相互参照の一貫性を確認する。
- 参照先不在または定義不整合があれば `Verdict: Fail` とし、`Rationale:` に具体的内容を列挙する。
- 対象文書が `docs/docs-governance.md` の正本・重複禁止規則に適合していることを確認する。
- 文書-only の補助記録参照は `docs/task-governance/workflow.md` に従って扱い、この skill でより厳格な自己 hash、exact file-set、台帳同期、current-cycle 文言一致要件を追加しない。
- 対象が `SKILL.md` の場合、frontmatter の `name`/`description`、`Required Reading Order` の有無と必要参照の網羅、正本重複禁止への適合を追加確認する。
- 厳密な追跡ファイル数、ファイル集合列挙、台帳同期、confirmation/review artifact 同期、current-cycle 文言一致をゲート条件にしない。
- レビュー担当の責務は判定返却のみ。ソース編集・コミット・実装は行わない。
- **レビュー独立性**: 過去記録や報告で代替せず、対象文書を直接確認して独立判定する。
- **再レビュー範囲**: 差し戻し後の再レビューでも前回セッションを持ち越さない。毎回独立セッションとして、正本が定める対象範囲を再確認する。
- 判定フォーマットは `docs/task-governance/implementation-review-judgement.md` を正本とする。ここに重複記載しない。

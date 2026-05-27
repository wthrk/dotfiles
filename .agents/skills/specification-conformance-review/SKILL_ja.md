---
name: specification-conformance-review
description: このスキルは、サブエージェントが仕様適合レビュー担当として割り当てられたときに、実装差分が作業項目の完了条件・違反是正対象・構造完了基準を満たしているかを検証するために使用する。
---

# Specification Conformance Review

## 役割

**仕様適合レビュー担当**

作業定義文書の `Violation Remediation Targets`・`Structural Completion Conditions`・`Completion Conditions` を現行コードへ直接照合する。各項目について現行ソースを開き、未解消が残っていないことを確認する。サマリーや実装担当報告で代替してはならない。

## 入力パラメーター

以下の **両方** を受け取る。

- 作業定義文書パス: `docs/tasks/<area>/work-items/<item>.md`
- レビュー対象コードパス

## 正本参照

- `docs/tasks/<area>/work-items/<item>.md`（active work item の作業定義文書）
- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/workflow.md`

## 必須読込順

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/tasks/README.md`
6. `docs/tasks/tasks.md`
7. `docs/tasks/<area>/work-items/<item>.md`（必須。自分で読む）
8. active work item が参照する領域固有資料（`docs/tasks/<area>/...`）
9. 関連する `docs/tasks/<area>/review-artifacts/...`

## ルール

- **必須の直接読解**: 作業定義文書を自分で読む。サマリー・実装担当報告・過去レビュー記録で代替しない。
- **直接コード照合**: 作業定義文書の `Violation Remediation Targets`・`Structural Completion Conditions`・`Completion Conditions` の各項目を、現行コードへ直接照合する。ビルド/テスト成功や過去記録で満足したと推定してはならない。
- 作業定義文書の各制約・完了条件・違反是正対象は個別に確認する。未確認や未解消があれば `Verdict: Fail` または `Verdict: Needs Fix` とし、`Rationale:` に列挙する。
- `cargo check` 成功、テスト成功、ビルド成功は代替にならない。`Pass` を返す前に、全完了条件を現行コードで追跡確認する。
- レビュー担当の責務は判定返却のみ。ソース編集・コミット・実装は行わない。修正は実装担当へ差し戻す。
- **レビュー独立性**: 過去記録や報告で代替せず、現行コードを直接確認して独立判定する。
- **再レビュー範囲**: 差し戻し後の再レビューでも前回セッションを持ち越さない。毎回独立セッションとして全対象を再確認する。
- 判定フォーマットは `docs/task-governance/implementation-review-judgement.md` を正本とする。ここに重複記載しない。`Rationale:` に確認項目と結果を明示する。

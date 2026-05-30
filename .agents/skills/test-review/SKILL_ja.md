---
name: test-review
description: このスキルは、サブエージェントがテストレビュー担当として割り当てられたときに、テストが作業項目の完了条件を網羅しているか、および test double / fixture が production source tree に混入していないかを検証するために使用する。
---

# Test Review

## 役割

**テストレビュー担当**

テストコードが仕様（作業定義文書の完了条件）を実際に検証していることを確認する。加えて、test double（Fake/Stub/Mock の定義）と fixture が production source tree に混入していないことを、形式だけでなく責務基準で判定する。

## 入力パラメーター

以下の **両方** を受け取る。

- 作業定義文書パス: `docs/tasks/<area>/work-items/<item>.md`
- レビュー対象コードパス

## 正本参照

- `docs/tasks/<area>/work-items/<item>.md`
- `docs/task-governance/implementation-review-judgement.md`
- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/architecture/review-checklist.md`

## 必須読込順

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/architecture/hexagonal-implementation-rules.md`（`tests/` 層配置規則確認）
4. `docs/architecture/review-checklist.md`（`tests/` 配下チェック項目確認）
5. `docs/task-governance/implementation-review-judgement.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. 渡された作業定義文書パス

## ルール

- **完了条件とテスト網羅**: 作業定義文書の `Completion Conditions` 各項目を検証するテストがあるか確認する。欠けがあれば `Verdict: Fail`。
- **責務基準判定**: test double 混入判定は、ファイル名や `#[cfg(test)]` / `#[cfg(feature = "...")]` / port trait 実装有無といった形式ではなく責務で行う。責務が当該層に属さなければ `Verdict: Fail`。
- **production 層への double 定義混入禁止**: 実依存の肩代わり型（Fake/Stub/Mock の定義）が `adapters/`・`application/`・`domain/`・`ports/`・`support/` など production 層にある場合は `Verdict: Fail`。`#[cfg(test)]`、feature gate、port trait 実装は免除にならない。`tests/` 層または test-support 専用 crate へ移す。
- **internal backend stub の例外**: adapter 配下の `secrets-internal-test-stub` feature 専用 backend stub は、`docs/architecture/hexagonal-implementation-rules.md` の canonical internal backend stub 条件と `docs/architecture/review-checklist.md` の責務チェックを満たす場合に限り許可する。条件をここで重複定義せず、正本を直接読んで満たしていなければ `Verdict: Fail`。
- **inline unit test は禁止対象外**: production 層 `src/` の通常の inline unit test は許可される。`#[test]` や `#[cfg(test)]` の存在だけで `Verdict: Fail` にしない。
- **直接コード確認**: 実ファイルを開いてテスト存在・配置を確認する。サマリーや報告で代替しない。
- **レビュー独立性**: 過去記録や報告で代替しない。対象コードを直接読んで独立判定する。
- **レビュー担当の責務範囲**: 判定返却のみ。ソース編集・コミットは行わない。修正は実装担当へ差し戻す。
- 判定フォーマットは `docs/task-governance/implementation-review-judgement.md` を正本とする。ここに重複記載しない。`Rationale:` に各確認項目と結果を列挙する。

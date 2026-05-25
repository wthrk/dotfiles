---
name: test-review
description: Use this skill when a subagent is assigned as the テストレビュー担当 to verify that tests cover the work item's completion conditions and that test doubles/fixtures are not mixed into the production source tree.
---

# Test Review

## 役割

**テストレビュー担当**

テストコードが仕様（作業定義文書の完了条件）を実際に検証しているかを確認する。また test double・fixture・stub が production source tree に混入していないことを確認する。

## 受け取るパラメーター

**作業定義文書パス**（`docs/tasks/<area>/work-items/<item>.md`）と**レビュー対象コードパス**の両方。

## Governing Sources

- `docs/tasks/<area>/work-items/<item>.md` (the active work item's work definition document) governs the specific completion conditions that tests must cover.
- `docs/task-governance/implementation-review-judgement.md` governs verdict format and aggregation rules.
- `docs/architecture/hexagonal-implementation-rules.md` governs layer boundaries, including the rules for `tests/` layer placement.
- `docs/architecture/review-checklist.md` governs per-directory check items including `tests/` layer checks.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/architecture/hexagonal-implementation-rules.md`（tests/ 層の配置ルール確認のため）
4. `docs/architecture/review-checklist.md`（tests/ 配下のチェック項目）
5. `docs/task-governance/implementation-review-judgement.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. 作業定義文書（渡されたパス）

## Rules

- **テストによる完了条件の網羅確認**: 作業定義文書の `完了条件` に列挙された各項目に対し、それを検証するテストが存在するかを確認する。テストが存在しない完了条件項目がある場合は `判定: 不合格` とする。
- **test double・fixture・stub の混入禁止**: `adapters/`・`application/` 等の production source tree に test double・fixture・stub が混入していないかを確認する。これらは `tests/` 層または `#[cfg(test)]` ブロック内にのみ存在してよい。
- **`#[cfg(test)]` 残存禁止**: `#[cfg(test)]` ラップを用いたテストコードが production ファイル（`adapters/`・`application/` 等）に残存している場合は `判定: 不合格` とする。テストは `tests/` 層に分離すること。
- **直接コード確認**: テストの存在・配置を確認するために実際のファイルを開いて確認する。サマリーや実装担当の報告で代替してはならない。
- **レビュー独立性**: 過去のレビュー記録・確認記録・実装担当の報告を参照して判定の代替にしてはならない。判定は必ず対象コードを直接読んで独立に行わなければならない。
- **レビュー担当の職責範囲**: レビュー担当はあくまで判定を返すのみである。ソースファイルを直接編集してはならず、コミット作業も行ってはならない。修正はすべて実装実行担当に差し戻すこと。
- 判定フォーマットは `docs/task-governance/implementation-review-judgement.md` に従う。判定フォーマットのルールをここに複製してはならない。`根拠:` に各確認項目とその結果を明示的に列挙すること。

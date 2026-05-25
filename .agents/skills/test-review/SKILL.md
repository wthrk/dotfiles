---
name: test-review
description: Use this skill when a subagent is assigned as the テストレビュー担当 to verify that tests cover the work item's completion conditions and that test doubles/fixtures are not mixed into the production source tree.
---

# Test Review

## 役割

**テストレビュー担当**

テストコードが仕様（作業定義文書の完了条件）を実際に検証しているかを確認する。また test double（Fake/Stub/Mock の定義）・fixture が production source tree に混入していないことを、形式ではなく責務基準で確認する。

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
- **責務基準の判定**: test double 混入の判定は、形式（ファイル名・`#[cfg(test)]` か `#[cfg(feature = "...")]` か・port trait を実装しているか）ではなく、コードの**責務**で行う。各シンボル・各ファイル・各 gate ブロックについて「責務は何か」「その責務はこの層に属すか」を問い、責務が層に属さなければ形式が正しくても `判定: 不合格` とする。判定基準の正本は `docs/architecture/review-checklist.md` の `tests/` 配下セクションと `責務基準の判定原則` であり、ここに複製しない。
- **test double 定義の production 層混入禁止**: 実依存をテスト用に肩代わりする型（Fake/Stub/Mock の**定義**）が production 層（`adapters/`・`application/`・`domain/`・`ports/`・`support/` 配下等）に存在する場合は `判定: 不合格` とする。`#[cfg(test)]` ラップ・`#[cfg(feature = "...")]` gate・port trait 実装はいずれもこの禁止の免除理由にならない。これらは `tests/` 層または専用 test-support crate に移動して解消する。
- **inline unit test は禁止対象外**: production 層の `src/` ファイル内の通常の inline unit test（`#[cfg(test)] mod tests { #[test] fn ... }`）はその module 自身の private 関数を検証する idiomatic な Rust であり、許可される。`#[test]` 関数や `#[cfg(test)]` ブロックの存在のみを理由に `判定: 不合格` としてはならない。禁止されるのは double の**定義**が production 層に置かれている場合に限る。判定の分かれ目は形式ではなく責務（その module 自身の検証か、実依存の肩代わり定義か）である。
- **直接コード確認**: テストの存在・配置を確認するために実際のファイルを開いて確認する。サマリーや実装担当の報告で代替してはならない。
- **レビュー独立性**: 過去のレビュー記録・確認記録・実装担当の報告を参照して判定の代替にしてはならない。判定は必ず対象コードを直接読んで独立に行わなければならない。
- **レビュー担当の職責範囲**: レビュー担当はあくまで判定を返すのみである。ソースファイルを直接編集してはならず、コミット作業も行ってはならない。修正はすべて実装実行担当に差し戻すこと。
- 判定フォーマットは `docs/task-governance/implementation-review-judgement.md` に従う。判定フォーマットのルールをここに複製してはならない。`根拠:` に各確認項目とその結果を明示的に列挙すること。

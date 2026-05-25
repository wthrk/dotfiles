---
name: documentation-review
description: Use this skill when a subagent is assigned as the ドキュメントレビュー担当 to verify that code documentation comments are consistent with the implementation and explain the "why" rather than just the "what".
---

# Documentation Review

## 役割

**ドキュメントレビュー担当**

コード内ドキュメントコメント（`///`・`//!`・`/** */` 等）が実装と整合しているかを確認する。また誤解を招くコメント・古くなったコメント・実装と矛盾するコメントを指摘する。

## 受け取るパラメーター

**レビュー対象コードパス**のみ。作業定義文書パスは渡されない。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md` governs verdict format and aggregation rules.
- `docs/docs-governance.md` (if present) governs documentation conventions that code comments must conform to.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/tasks/README.md`
5. `docs/tasks/tasks.md`
6. `docs/docs-governance.md`（存在する場合）

## Rules

- **実装との整合確認**: ドキュメントコメントの記述が実際の実装と矛盾していないかを確認する。関数シグネチャ・型・返却値・副作用についての記述が現行コードと一致していること。
- **Why の説明確認**: コメントが「何をするか（what）」の説明にとどまらず「なぜ（why）」を説明しているかを確認する。what のみを繰り返すコメントは不十分とする。
- **文書規約への適合**: `docs/docs-governance.md` が存在する場合は、ドキュメントコメントがその規約に適合しているかを確認する。
- **誤解を招くコメントの指摘**: 誤解を招くコメント・古くなったコメント・実装と矛盾するコメントを `根拠:` に列挙する。
- **直接コード確認**: 対象コードパスのファイルを直接開いて確認する。サマリーや実装担当の報告で代替してはならない。
- **レビュー独立性**: 過去のレビュー記録・確認記録・実装担当の報告を参照して判定の代替にしてはならない。判定は必ず対象コードを直接読んで独立に行わなければならない。
- **レビュー担当の職責範囲**: レビュー担当はあくまで判定を返すのみである。ソースファイルを直接編集してはならず、コミット作業も行ってはならない。修正はすべて実装実行担当に差し戻すこと。
- 判定フォーマットは `docs/task-governance/implementation-review-judgement.md` に従う。判定フォーマットのルールをここに複製してはならない。`根拠:` に各確認項目とその結果を明示的に列挙すること。

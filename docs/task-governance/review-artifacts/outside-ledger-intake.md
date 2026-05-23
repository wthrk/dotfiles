# outside-ledger-intake

このファイルは、領域台帳に未収載の作業を実行する前に、分類と責任境界を記録する。

## 記録テンプレート

- 実施日:
- 対象依頼:
- 分類 (`task-list-outside` 固定):
- 責任境界:
- 対象差分識別子（着手時に未確定なら `未確定`）:
- レビュー記録の保存先:
- 備考（任意）:

## 2026-05-22 task-governance / secret-recovery docs simplification

- 実施日: `2026-05-22`
- 対象依頼: `類似する過剰な規則を active な導線から除去し、レビューしてコミットして別プロセス実行確認まで回す`
- 分類 (`task-list-outside` 固定): `task-list-outside`
- 責任境界: `active work item YubiKey そのものの実装進捗は変更せず、task-governance / task ledgers / secret-recovery の active な文書導線だけを是正する`
- 対象差分識別子（着手時に未確定なら `未確定`）: `2026-05-22-task-governance-doc-simplification`
- レビュー記録の保存先: `docs/task-governance/review-artifacts/documentation-remediation-2026-05-22.md`
- 備考（任意）: `YubiKey の 2026-05-21 dry-run 記録は履歴として分離し、current confirmation/review は未着手テンプレートへ戻した`

## 2026-05-23 yubikey work-item 定義文書整備

- 実施日: `2026-05-23`
- 対象依頼: `yubikey.md の規約違反解消対象を抽象4項目から V1〜V16 の具体16件リスト（ファイルパス・規則参照・解消操作方向付き）に置換し、完了の判定条件・レビュー合格条件・違反ファイルマップを追加する。implementation-guidelines.md に縮退完了報告禁止ルールを追記する。`
- 分類 (`task-list-outside` 固定): `task-list-outside`
- 責任境界: `docs/tasks/secret-recovery/work-items/yubikey.md` と `docs/secret-recovery/implementation-guidelines.md` の2ファイルのみ。active work item（Bitwarden Secrets Manager）の実装進捗は変更しない。`tasks.md`・`issue-11-progress.md`・`review.md`・`confirmation.md` には触れない。
- 対象差分識別子（着手時に未確定なら `未確定`）: `2026-05-23-yubikey-workitem-doc-remediation`
- レビュー記録の保存先: `docs/task-governance/review-artifacts/outside-ledger-intake.md`（本記録をもって最小記録とする）
- 備考（任意）: 前セッションで `yubikey.md` と `implementation-guidelines.md` の更新を実施済み。本記録はその事後記録である。

## 2026-05-23 workflow サイクル分離ルール追記

- 実施日: `2026-05-23`
- 対象依頼: `Serena memory の Codex 調査結果（progress-0064）が示す8件の再構成要件を現行ドキュメントに対して充足確認し、未充足の要件7（過去/current サイクル分離）を workflow.md §5 に追記する`
- 分類 (`task-list-outside` 固定): `task-list-outside`
- 責任境界: `docs/task-governance/workflow.md` の §5「記録先」末尾へのサイクル分離ルール追記のみ。active work item（Bitwarden Secrets Manager）の実装進捗・`tasks.md` の状態は変更しない。
- 対象差分識別子（着手時に未確定なら `未確定`）: `2026-05-23-workflow-cycle-separation-rule`
- レビュー記録の保存先: `docs/task-governance/review-artifacts/outside-ledger-intake.md`（本記録をもって最小記録とする）
- 備考（任意）: 要件1〜6・8は既存ドキュメントで充足済みと確認。要件7のみ未充足であったため workflow.md に追記。yubikey.md は差戻し条件・実装順序ガイド・違反ファイルマップが追加済みで要件4・8も充足済みを確認。

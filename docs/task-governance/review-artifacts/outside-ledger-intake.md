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

## 2026-05-24 ドキュメントゼロベース再設計・skill/AGENTS.md 作り直し

- 実施日: `2026-05-24`
- 対象依頼: `現行ドキュメント群の問題点を確認し、ゼロベース再設計を行う。skill も AGENTS.md も作り直す。Codex に「次のタスクを実行しろ」とだけ指示した時に adapters 層の port 実装以外の公開禁止を含む完全なリファクタリングが実行されること、レビューでアーキテクチャ違反が棄却されることを確認できることがゴール。`
- 分類 (`task-list-outside` 固定): `task-list-outside`
- 責任境界: `docs/architecture/`、`.agents/skills/`、`docs/secret-recovery/implementation-guidelines.md`、`docs/task-governance/`、ルートの `AGENTS.md` / `AGENTS_ja.md`。active work item（Bitwarden Secrets Manager）の実装進捗・`docs/tasks/tasks.md` の作業項目状態は変更しない。
- 対象差分識別子（着手時に未確定なら `未確定`）: `2026-05-24-doc-zero-base-redesign`
- レビュー記録の保存先: `docs/task-governance/review-artifacts/outside-ledger-intake.md`（本記録および後続レビュー記録）
- 備考（任意）: 問題点の中核は (1) hexagonal 層とディレクトリ構成の対応が明示されていない、(2) V1〜V16 違反定義がファイル名固定で層ベースルールになっていない、(3) skills が Codex に完全な実行手順を提供していない、の3点。

### 2026-05-24 実装確認（2026-05-24-doc-zero-base-redesign）

- 変更したファイル:
  - `docs/architecture/hexagonal-implementation-rules.md`（「secrets モジュール構成」セクション追加）
  - `docs/tasks/secret-recovery/work-items/yubikey.md`（V1〜V16 に層違反ラベル追加、違反判定基準の冒頭文追加）
  - `.agents/skills/dotfiles-task-governance/SKILL.md`（Required Reading Order #9 追加、Rules 2件追加）
  - `.agents/skills/implementation-execution/SKILL.md`（Governing Sources 1件追加、Required Reading Order #5 追加、Rules 3件追加）
  - `.agents/skills/implementation-review-judgement/SKILL.md`（Governing Sources 1件追加、Required Reading Order #6 追加、Rules 3件追加）
  - `AGENTS.md`（Architecture Constraints セクション追加）
  - `AGENTS_ja.md`（アーキテクチャ制約セクション追加）
- 確認コマンド: `find /Users/ya/works/dotfiles/docs -name "*.md" | xargs grep -l "adapters.*port" | head -20`
- 結果: `/Users/ya/works/dotfiles/docs/tasks/secret-recovery/work-items/yubikey.md`、`/Users/ya/works/dotfiles/docs/architecture/hexagonal-implementation-rules.md`、`/Users/ya/works/dotfiles/docs/task-governance/review-artifacts/outside-ledger-intake.md`
- 差戻し修正: `.agents/skills/dotfiles-task-governance/SKILL.md` L29 の日本語括弧注釈を英語に修正（参照整合レビュー担当の指摘による）

## 2026-05-25 ドキュメント遵守失敗分析と文書整備

- 実施日: `2026-05-25`
- 対象依頼: `前回セッション（メインサブ含む）でドキュメント遵守に至らなかった原因をファイルから確認し、正しい哲学のもとに実装とレビューを行うようにドキュメントを整備修正する`
- 分類 (`task-list-outside` 固定): `task-list-outside`
- 責任境界: `docs/task-governance/`、`.agents/skills/` 配下の関連 SKILL.md、`docs/tasks/secret-recovery/review-artifacts/_review-template.md`。active work item（Bitwarden Secrets Manager）の実装進捗・`docs/tasks/tasks.md` の作業項目状態は変更しない。
- 対象差分識別子（着手時に未確定なら `未確定`）: `未確定`
- レビュー記録の保存先: `docs/task-governance/review-artifacts/outside-ledger-intake.md`（本記録および後続レビュー記録）
- 備考（任意）: 根本原因はオーケストレーター自身による必須レビュー役割スキップとself-execution。文書に記載されているにもかかわらず、サブエージェントが起動時に確実に遵守しなかった。文書の何が不十分だったかを調査し是正する。

### 2026-05-24 レビュー集約（2026-05-24-doc-zero-base-redesign）

- 運用整合レビュー担当: `判定: 合格` / 判定要約: 所見なし
- 参照整合レビュー担当（初回）: `判定: 要修正` / 判定要約: `dotfiles-task-governance/SKILL.md` L29 単一言語規則違反
- 参照整合レビュー担当（再レビュー）: `判定: 合格` / 判定要約: 所見なし
- 集約後レビュー判定: `合格`
- 集約根拠: 必須レビュー担当（運用整合・参照整合）全員が合格。差戻し修正後に再レビュー実施済み。

## 2026-05-30 PR #33 / Issue #30 secrets structure branch 作り直し記録

- 実施日: `2026-05-30`
- 対象依頼: `PR #32 を close し、origin/main 先頭から PR #33 / branch refactor/secrets-structure-issue-30-main として secrets structure 整理差分を作り直した状態の監査証跡を固定する`
- 分類 (`task-list-outside` 固定): `task-list-outside`
- 責任境界: `PR #33 / Issue #30 の branch 作り直し、対象差分、確認結果、レビュー状況、commit linkage の追跡記録に限定する。Bitwarden Secrets Manager 作業項目の Hypatia 以前の current-cycle 記録を PR #33 の合格根拠として再利用せず、root/area ledger の active item 選定や完了状態は変更しない。`
- 対象差分識別子（着手時に未確定なら `未確定`）: `PR #33 / branch refactor/secrets-structure-issue-30-main / base 5ff5e54 / 実装/レビュー対象終端 77dc03c / diff range 5ff5e54..77dc03c`（`11ff088` は直前 P1 対応 commit、`77dc03c` は fresh review 差し戻し（構造・PTY・追跡更新）対応 commit。文書-only 補正後の実際の HEAD は `git log` の HEAD で確認する）
- レビュー記録の保存先: `docs/tasks/secret-recovery/review-artifacts/bitwarden-secrets-manager/review.md`（PR #33 / Issue #30 task-list-outside 追跡節）
- 備考（任意）: `PR #32 は closed、PR #33 は open として作り直された前提の記録。PR #33 は origin/main 先頭 5ff5e54 からの作り直し commit 2ececf1 に、差戻し補正 commit ffe9880 / 7320c55 / fbc5096 / fa396f3 / ae1b917 / 97748c4 / 5e21afb / 4cd47d4 / 11ff088 / 77dc03c を重ねた状態として git log / git diff で確認した。ae1b917 は PR #33 証跡同期、97748c4 は BSM 対象コードパス漏れ指摘への対応、5e21afb は PR #33 現行 HEAD 証跡更新、4cd47d4 は削除済み adapter root を現行対象パス扱いしない過去補正時点、11ff088 は直前 P1 対応、77dc03c は fresh review 差し戻し（構造・PTY・追跡更新）対応。fresh review/集約/commit gate 確定前にユーザー依頼の PR AI review 対応として一部 PR review comment への返信または resolve を先行実施したが、これは PR 運用記録であり、fresh review 合格、集約合格、commit gate 充足、最終完了扱いの根拠にはしない。fresh review 未実施・集約未確定の状態は維持し、本記録はレビュー合格や commit gate 充足の代替ではない。`

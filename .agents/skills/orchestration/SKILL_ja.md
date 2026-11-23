---
name: orchestration
description: このリポジトリの top-level タスク実行依頼で、メインエージェントが作業単位選定と役割委譲を統治規約に従って行う場合に使う。
---

# Orchestration

## Actor Binding

このスキル有効時の現在アクターは **orchestrator**。

## Governing Sources

- `docs/task-governance/workflow.md`
- `docs/task-governance/implementation-review-judgement.md`
- `docs/task-governance/security-obligations.md`
- `docs/docs-governance.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/security-obligations.md`
6. `docs/docs-governance.md`
7. ユーザー指定の GitHub issue、PR、または明示タスク
8. `docs/README.md` から対象領域の README と正本文書まで辿り、タスクで明示されていない文書も読む。パッケージの追加・再導入・更新では、委譲前に `README.md` の「更新と適用」を読み、`docs/automation/README.md` から nightly lock-bump と Homebrew cask-pinning の文書まで辿る。

## Rules

- ユーザー指定の GitHub issue、PR、または明示タスクから作業単位を 1 件だけ確定する。
- 役割起動に必要な委譲パラメーターだけを抽出する。
- 委譲前に、それらの正本文書から既存の更新・適用経路を把握し、依頼範囲がリポジトリの宣言変更、commit / PR / main を通じた配送、実機への適用のどこまでかを委譲内容に含める。このリポジトリでのパッケージ再導入依頼は、ユーザーが実機適用を求めない限り宣言変更として扱い、代わりに直接インストールを委譲しない。
- 必要な fresh role agent を起動し、実装・レビュー・完了判定・テスト・ビルド・ファイル編集を自己実行しない。
- サブエージェントの実行中にポーリングしたり、その transcript やログを読んだりしない。コンテキスト分離を保つため、イベント駆動の完了通知・メッセージ通知を待つ。
- 依頼がすでにタスク実行コマンドの場合、委譲可否の追加許可を求めない。
- 必要役割を起動できない場合だけ、起動/利用失敗を記録する。
- 詳細禁止事項と branch / PR gate は `docs/task-governance/workflow.md` が所有する。

---
name: task-completion-judgement
description: サブエージェントが完了条件と必須エビデンスに基づいて作業項目が完了へ遷移できるかを決定しなければならないときに使用するスキル。
---

# タスク完了判定

## 統括文書

- `docs/task-governance/workflow.md`: 役割割当、フォールバック、サブエージェント割当を統括する。
- `docs/task-governance/implementation-review-judgement.md`: 必須レビュー役割と、完了判定に必要なレビュー合格の集約依存を統括する。
- `docs/task-governance/task-completion-judgement.md` および `docs/task-governance/progress-judgement.md`: 完了条件とエビデンス要件を統括する。

## 必須参照順

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/task-completion-judgement.md`
6. `docs/task-governance/progress-judgement.md`
7. `docs/task-governance/security-obligations.md`
8. `docs/tasks/README.md`
9. `docs/tasks/tasks.md`
10. アクティブ作業項目が要求する領域固有成果物（`docs/tasks/<area>/...`）
11. `docs/tasks/<area>/review-artifacts/` 配下の関連する確認・レビュー成果物

`docs/tasks/<area>/tasks.md` はアクティブ作業項目が明示的に参照している場合にのみ必須とする。

## 起動タイミング

このスキルは確認・レビューステージ後の `完了` 遷移判定に使用する。

アクター拘束: 本スキルが有効な間、現在の実行者は上記統括文書に拘束されたタスク完了判定役割のみとなる。

## 規則

- 完了のみを判定すること。レビュー開始または集約判定を再実行してはならない。
- 完了を承認する前に、同一変更セットからのエビデンスの完全性を要求すること。
- `docs/tasks/tasks.md` からアクティブ項目を選定し、完了対象がその項目と整合していることを検証し、その項目が参照するタスク定義および確認・レビュー成果物を実行統括ソースとして従うこと。

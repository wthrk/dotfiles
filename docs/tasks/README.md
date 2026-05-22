# tasks

このディレクトリは、リポジトリ全体の root 台帳と、active work item が参照する作業定義/証跡を配置する。

## 配下の項目

- [tasks.md](tasks.md): repository-wide root active ledger（単一のタスク入口）を定義する。
- [secret-recovery/README.md](secret-recovery/README.md): secret-recovery のタスク群を案内する。
- [repo-governance/README.md](repo-governance/README.md): repo-global ガバナンス文書整合のタスク群を案内する。

## タスク台帳の探索

- [tasks.md](tasks.md) は単一のタスク入口かつ active work item の選定正本であり、継続依頼・進捗依頼を含む全依頼で最初に読む。
- [tasks.md](tasks.md) の `現在の作業項目` で active work item を 1 件確定し、その項目が要求する参照先（`docs/tasks/<area>/README.md`、`docs/tasks/<area>/tasks.md`、`work-items/` など）を実行統治源として読む。
- 特定領域に限定されない依頼でも、`docs/tasks/tasks.md` を起点に対象領域を確定する。
- 依頼内容が root active ledger と、active work item が要求する参照先のいずれにも存在しない場合は、実行前に `docs/task-governance/workflow.md` の `task-list-outside` 判定を行い、`docs/task-governance/review-artifacts/outside-ledger-intake.md` の最小記録を残す。
- `task-list-outside` の着手前要件と `S0 -> S1` 遷移条件は `docs/task-governance/workflow.md` を正本として参照する。

## 配置原則

- `docs/tasks/tasks.md`: repository-wide root active ledger（`現在の作業項目` の選定正本）
- `docs/tasks/<area>/tasks.md`: active work item が参照している場合に必ず読む実行統治台帳/履歴/状態
- `docs/tasks/<area>/work-items/`: 各作業項目の仕事の定義
- `docs/tasks/<area>/issue-*.md`: 粗粒度 issue / phase 進捗
- `docs/tasks/<area>/review-artifacts/`: 確認証跡とレビュー記録
- `task-list-outside` 判定の作業記録先: `docs/task-governance/review-artifacts/`

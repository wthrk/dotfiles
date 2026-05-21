# tasks

このディレクトリは、リポジトリ全体のタスク台帳、作業定義、粗粒度進捗、証跡を領域ごとに配置する。

## 配下の項目

- [secret-recovery/README.md](secret-recovery/README.md): secret-recovery のタスク群を案内する。
- [repo-governance/README.md](repo-governance/README.md): repo-global ガバナンス文書整合のタスク群を案内する。

## タスク台帳の探索

- 継続依頼・進捗依頼では、まず領域 README を選び、対象領域の `tasks.md` を台帳として読む。
- 特定領域に限定されない依頼では、`docs/tasks/<area>/README.md` を起点に対象台帳を確定する。
- 依頼内容がどの領域台帳にも存在しない場合は、実行前に `docs/task-governance/workflow.md` の `task-list-outside` 判定を行い、`docs/task-governance/review-artifacts/outside-ledger-intake.md` の最小記録を残す。
- `task-list-outside` の着手前要件と `S0 -> S1` 遷移条件は `docs/task-governance/workflow.md` を正本として参照する。

## 配置原則

- `docs/tasks/<area>/tasks.md`: その領域の進捗台帳
- `docs/tasks/<area>/work-items/`: 各作業項目の仕事の定義
- `docs/tasks/<area>/issue-*.md`: 粗粒度 issue / phase 進捗
- `docs/tasks/<area>/review-artifacts/`: 確認証跡とレビュー記録
- `task-list-outside` 判定の作業記録先: `docs/task-governance/review-artifacts/`

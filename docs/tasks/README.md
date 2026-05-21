# tasks

このディレクトリは、リポジトリ全体のタスク台帳、作業定義、粗粒度進捗、証跡を領域ごとに配置する。

## 配下の項目

- [secret-recovery/README.md](secret-recovery/README.md): secret-recovery のタスク群を案内する。

## タスク台帳の探索

- 継続依頼・進捗依頼では、まず領域 README を選び、対象領域の `tasks.md` を台帳として読む。
- 特定領域に限定されない依頼では、`docs/tasks/<area>/README.md` を起点に対象台帳を確定する。

## 配置原則

- `docs/tasks/<area>/tasks.md`: その領域の進捗台帳
- `docs/tasks/<area>/work-items/`: 各作業項目の仕事の定義
- `docs/tasks/<area>/issue-*.md`: 粗粒度 issue / phase 進捗
- `docs/tasks/<area>/review-artifacts/`: 確認証跡とレビュー記録

# task-governance

このディレクトリは、リポジトリ全体で共有するタスク運用規約の入口である。

## 配下の項目

- [workflow.md](workflow.md#タスク運用ワークフロー): オーケストレーション、実装、レビュー、進捗判定、履歴復元の役割分担と進め方を定義する。
- [implementation-execution.md](implementation-execution.md#実装実行規則): 実装担当が守るべき必須参照、再読対象、記録義務を定義する。
- [implementation-review-judgement.md](implementation-review-judgement.md#実装レビュー判定): レビュー開始条件、レビュー担当責務、集約規則を定義する。
- [task-completion-judgement.md](task-completion-judgement.md#タスク完了判定): 作業項目 `完了` の判定条件を定義する。
- [progress-judgement.md](progress-judgement.md#進捗判定規則): 状態遷移、前進条件、証跡要件を定義する。
- [security-obligations.md](security-obligations.md#セキュリティ義務): 実装・確認・レビュー・進捗更新で共通するセキュリティ義務を定義する。
- [task-file-contract.md](task-file-contract.md#タスクファイルに必須の項目): タスクファイルが持つべき項目、台帳と作業定義文書の責務分離、重複禁止を定義する。
- [legacy-issue-tracking.md](legacy-issue-tracking.md#復元規則): 粗粒度 issue / phase 進捗の保持と復元の扱いを定義する。

## 参照順

1. 作業全体の進め方は [workflow.md](workflow.md#タスク運用ワークフロー) を参照する。
2. 実装担当の義務は [implementation-execution.md](implementation-execution.md#実装実行規則) を参照する。
3. レビュー開始と集約は [implementation-review-judgement.md](implementation-review-judgement.md#実装レビュー判定) を参照する。
4. 完了判定は [task-completion-judgement.md](task-completion-judgement.md#タスク完了判定) を参照する。
5. 状態更新は [progress-judgement.md](progress-judgement.md#進捗判定規則) を参照する。
6. セキュリティ義務は [security-obligations.md](security-obligations.md#セキュリティ義務) を参照する。
7. タスクファイル構造は [task-file-contract.md](task-file-contract.md#タスクファイルに必須の項目) を参照する。
8. 粗粒度の issue / phase 進捗がある場合は [legacy-issue-tracking.md](legacy-issue-tracking.md#復元規則) を参照する。

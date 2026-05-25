# task-governance

このディレクトリは、リポジトリ共通のタスク運用規約をまとめる入口である。

## 配下の項目

- [workflow.md](workflow.md#タスク運用ワークフロー): 最小フロー、状態、コミット着手ゲート、ブランチ・コミット・プルリクエスト運用。
- [implementation-execution.md](implementation-execution.md#実装実行規則): 実装担当の実行規則。
- [implementation-review-judgement.md](implementation-review-judgement.md#実装レビュー判定): 必須レビュー役割と集約規則。
- [progress-judgement.md](progress-judgement.md#進捗判定規則): 進捗前進の最小証跡要件。
- [task-completion-judgement.md](task-completion-judgement.md#タスク完了判定): 完了判定とコミット許可条件。
- [task-file-contract.md](task-file-contract.md#タスクファイル契約): 台帳の最小必須項目。
- [security-obligations.md](security-obligations.md#セキュリティ義務): セキュリティ共通義務。

## 運用原則

- 主記録は `変更差分` と `レビュー結果` とする。
- 同じ事実の多重同期は要求しない。
- 文書是正では、無関係な台帳や粗粒度進捗の更新を必須にしない。

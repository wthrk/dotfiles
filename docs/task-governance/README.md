# task-governance

このディレクトリは、リポジトリ共通のタスク運用規約をまとめる入口である。

## 配下の項目

- [workflow.md](workflow.md): タスク実行フロー、役割分離、委譲、コミット/PR 運用。
- [implementation-execution.md](implementation-execution.md): 実装担当の実行規則。
- [implementation-review-judgement.md](implementation-review-judgement.md): 必須レビュー役割と集約規則。
- [progress-judgement.md](progress-judgement.md): 進捗前進の最小根拠要件。
- [task-completion-judgement.md](task-completion-judgement.md): 完了判定とコミット許可条件。
- [security-obligations.md](security-obligations.md): セキュリティ共通義務。
- [pr-mergeability-loop.md](pr-mergeability-loop.md): PR review 対応、checks、mergeability 確認を反復して PR を merge 可能状態にする運用。

## 運用原則

- 作業単位は、ユーザーが指定した GitHub issue、PR、または明示タスクである。
- 主記録は変更差分、実検証、レビュー結果、PR 上の review thread 対応とする。
- repo 内に完了済みタスク台帳や review artifact を再掲しない。

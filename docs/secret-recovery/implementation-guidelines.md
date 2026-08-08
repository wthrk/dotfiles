# secret-recovery implementation guidelines

この文書は、secret-recovery 領域に固有の実装方針を定義する恒久文書である。過去 issue / PR / review cycle の履歴は保持しない。

## 参照入口

- 共通フロー: [../task-governance/workflow.md](../task-governance/workflow.md)
- 実装担当規則: [../task-governance/implementation-execution.md](../task-governance/implementation-execution.md)
- レビュー担当と集約: [../task-governance/implementation-review-judgement.md](../task-governance/implementation-review-judgement.md)
- セキュリティ義務: [../task-governance/security-obligations.md](../task-governance/security-obligations.md)
- アーキテクチャ規約: [../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md)
- secret handling: [secret-handling.md](secret-handling.md)
- 機能仕様: [secret-recovery-spec.md](secret-recovery-spec.md)
- 初期プロビジョニング runbook: [initial-provisioning-runbook.md](initial-provisioning-runbook.md)

## 実装単位

secret-recovery の作業は、ユーザー指定の GitHub issue、PR、または明示タスクを作業単位とする。作業単位ごとに、対象となる仕様・設計文書、対象コードパス、完了条件、検証条件を委譲入力として固定する。

実装単位は次のいずれかに分類する。

- `仕様・設計更新`: `docs/secret-recovery/` 配下の恒久仕様、設計、runbook、secret handling を更新する。
- `実装`: Rust/Nix/shell 等の実装差分と対応テストを更新する。
- `文書整合`: 恒久文書間の参照、用語、責務境界を整える。
- `PR review remediation`: PR review thread の指摘に対して修正または不採用理由を示す。

## 役割境界

- オーケストレーターは、指定作業単位と委譲パラメーターを確定し、必要役割を起動する。
- 実装担当は、指定された仕様・設計・対象パスを直接読み、差分と検証結果を作る。
- レビュー担当は、対象差分と指定仕様を直接読み、担当観点の判定を返す。
- 完了判定担当は、対象差分、検証結果、レビュー結果、PR review thread 対応状態を根拠に判定する。

## 実装方針

- secret の平文は CLI 引数、ログ、エラー本文、一時ファイル、review 記録へ残さない。
- secret の保護境界、core dump 抑止、plaintext buffer の借用/所有境界は [secret-handling.md](secret-handling.md) を正本とする。
- BWS / YubiKey / GPG / Bitwarden Password Manager の保存モデルと責務分担は、各設計文書と [secret-recovery-spec.md](secret-recovery-spec.md) を正本とする。
- 実装は現行の hexagonal layer boundary に従う。層責務、依存方向、公開面は [../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md) を適用する。
- test double / fixture の配置は責務で判断する。形式や feature gate だけで許可または禁止を決めない。

## 確認とレビュー

- 実装担当は、変更後の対象差分に対して必要な確認だけを行い、完了報告に対象差分識別子、コマンド、結果、未実施理由を記録する。
- executable behavior を含む変更では、関連する unit / integration / static check を選ぶ。
- 文書のみの変更では、リンク・参照整合と `git diff --check` を基本確認とする。利用者が明示した場合は指定検証を実行する。
- PR review thread がある場合は、採用/不採用の返信、修正 commit、resolve 状態を完了条件に含める。

## 完了条件

secret-recovery の作業単位は、次を満たす場合に完了候補となる。

- 指定 issue / PR / 明示タスクの完了条件を満たしている。
- 必要な実検証が実施され、結果または未実施理由が記録されている。
- 必須レビュー担当が全員 `合格` し、集約後レビュー判定が `合格` である。
- 未解決の security finding、secret exposure、権限境界逸脱が残っていない。
- PR が対象の場合、AI review を含む全 review thread へ対応し、resolve されている。

# 新規マシン秘密情報復旧基盤タスク

この文書は secret-recovery の進捗台帳である。進め方は [../../task-governance/workflow.md](../../task-governance/workflow.md#タスク運用ワークフロー)、固定実装単位は [../../secret-recovery/implementation-guidelines.md](../../secret-recovery/implementation-guidelines.md#計画依頼の固定実装単位) を参照する。

## 作業項目一覧

### YubiKey

- 状態: `進行中`
- 現行サイクル状態: `再レビュー待ち`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/yubikey.md](work-items/yubikey.md#12-yubikey-秘密情報保存)
- レビュー記録: [review-artifacts/yubikey/review.md](review-artifacts/yubikey/review.md#yubikey-レビュー記録)（現行サイクル状態: 再レビュー待ち）
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_*.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/domain.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/domain/model.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/aead.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/oaep.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
  - `rust/dotfiles-cli/Cargo.toml`
- 実装状態: `実装中`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/yubikey.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 完了 | `docs/secret-recovery/yubikey-secret-storage-design.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 実装 ステップ1: V8,V16（domain SecretDevice→ports移設・io::Write除去） | 完了 | 実コード差分 | [work-items/yubikey.md#実装順序ガイド推奨](work-items/yubikey.md#実装順序ガイド推奨) |
| 実装 ステップ2: V9（use case outcome 型の domain 統一） | 完了 | 実コード差分 | [work-items/yubikey.md#実装順序ガイド推奨](work-items/yubikey.md#実装順序ガイド推奨) |
| 実装 ステップ3: V6,V7（port DTO/parser/prompt除去・support依存除去） | 完了 | 実コード差分 | [work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26](work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ4: V10（責務再分離） | 完了 | 実コード差分 | [work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26](work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ5: V11,V12,V13（adapter面整理） | 完了 | 実コード差分 | [work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26](work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ6: V4,V5（application配下adapter移設） | 完了 | 実コード差分 | [work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26](work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ7: V1,V2,V3（application concrete I/O依存除去） | 完了 | 実コード差分 | [work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26](work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ8: V14,V15（same-route維持 + stub配置/責務整合） | 完了 | 実コード差分 | [work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26](work-items/yubikey.md#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 確認 | 再レビュー待ち | `review-artifacts/yubikey/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 再レビュー待ち | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 修正済み | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### Bitwarden Secrets Manager

- 状態: `未開始`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/bitwarden-secrets-manager.md](work-items/bitwarden-secrets-manager.md#13-bitwarden-secrets-manager-クライアント)
- レビュー記録: [review-artifacts/bitwarden-secrets-manager/review.md](review-artifacts/bitwarden-secrets-manager/review.md#bitwarden-secrets-manager-レビュー記録)
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/domain/model.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 実装状態: `未実装`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/bitwarden-secrets-manager.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 進行中 | `docs/secret-recovery/bitwarden-secrets-manager-design.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 確認 | 未着手 | `review-artifacts/bitwarden-secrets-manager/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未着手 | `review-artifacts/bitwarden-secrets-manager/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手 | `review-artifacts/bitwarden-secrets-manager/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### GnuPG / SSH

- 状態: `未開始`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/gnupg-ssh.md](work-items/gnupg-ssh.md)
- レビュー記録: [review-artifacts/gnupg-ssh/review.md](review-artifacts/gnupg-ssh/review.md)
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 実装状態: `未実装`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/gnupg-ssh.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 進行中 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 確認 | 未着手 | `review-artifacts/gnupg-ssh/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未着手 | `review-artifacts/gnupg-ssh/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手 | `review-artifacts/gnupg-ssh/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### Git

- 状態: `未開始`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/git.md](work-items/git.md#15-password-store-復元)
- レビュー記録: [review-artifacts/git/review.md](review-artifacts/git/review.md#git-レビュー記録)
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 実装状態: `未実装`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/git.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 進行中 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 確認 | 未着手 | `review-artifacts/git/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未着手 | `review-artifacts/git/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手 | `review-artifacts/git/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### Bitwarden Password Manager

- 状態: `未開始`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/bitwarden-password-manager.md](work-items/bitwarden-password-manager.md#16-bitwarden-password-manager-cli-ログイン)
- レビュー記録: [review-artifacts/bitwarden-password-manager/review.md](review-artifacts/bitwarden-password-manager/review.md#bitwarden-password-manager-レビュー記録)
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 実装状態: `未実装`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/bitwarden-password-manager.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 進行中 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 確認 | 未着手 | `review-artifacts/bitwarden-password-manager/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未着手 | `review-artifacts/bitwarden-password-manager/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手 | `review-artifacts/bitwarden-password-manager/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### 新規マシン復旧フロー統合

- 状態: `未開始`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/integration.md](work-items/integration.md#17-新規マシン復旧フロー統合)
- レビュー記録: [review-artifacts/integration/review.md](review-artifacts/integration/review.md)
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 実装状態: `未実装`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 未着手 | `work-items/integration.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 未着手 | `work-items/integration.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 未着手 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 確認 | 未着手 | `review-artifacts/integration/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未着手 | `review-artifacts/integration/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手 | `review-artifacts/integration/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

## 移管履歴

- `2026-05-21`: `最終ドキュメント整理` は secret-recovery の通常作業項目から除外し、repo-global の文書整合作業として [../repo-governance/tasks.md](../repo-governance/tasks.md#ガバナンス文書整合) へ移管した。関連する台帳・作業定義・確認/レビュー証跡の正本は repo-governance 側を参照する。

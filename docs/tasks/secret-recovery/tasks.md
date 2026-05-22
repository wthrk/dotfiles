# 新規マシン秘密情報復旧基盤タスク

この文書は secret-recovery の進捗台帳である。進め方は [../../task-governance/workflow.md](../../task-governance/workflow.md#タスク運用ワークフロー)、固定実装単位は [../../secret-recovery/implementation-guidelines.md](../../secret-recovery/implementation-guidelines.md#計画依頼の固定実装単位) を参照する。

## 作業項目一覧

### YubiKey

- 状態: `未開始`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/yubikey.md](work-items/yubikey.md#12-yubikey-秘密情報保存)
- レビュー記録: [review-artifacts/yubikey/review.md](review-artifacts/yubikey/review.md#yubikey-レビュー記録)
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/storage_service.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/domain/model.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 過去サイクル実行記録（履歴・次サイクル再利用禁止）:
  - 実装担当: `impl-agent-yubikey-dryrun`
  - 実装担当 agent/run 識別子: `agent:impl-yubikey-dryrun / run:2026-05-21-yubikey-impl-001`
  - レビュー担当一覧:
    - 構造レビュー担当: `reviewer-structure-yubikey-dryrun`
    - 構造レビュー担当 agent/run 識別子: `agent:review-structure-yubikey / run:2026-05-21-yubikey-rs-001`
    - 運用整合レビュー担当: `reviewer-ops-yubikey-dryrun`
    - 運用整合レビュー担当 agent/run 識別子: `agent:review-ops-yubikey / run:2026-05-21-yubikey-ro-001`
    - セキュリティレビュー担当: `reviewer-security-yubikey-dryrun`
    - セキュリティレビュー担当 agent/run 識別子: `agent:review-security-yubikey / run:2026-05-21-yubikey-rsec-001`
    - 仕様適合レビュー担当: `reviewer-spec-yubikey-dryrun`
    - 仕様適合レビュー担当 agent/run 識別子: `agent:review-spec-yubikey / run:2026-05-21-yubikey-rsp-001`
    - 参照整合レビュー担当: `reviewer-reference-yubikey-dryrun`
    - 参照整合レビュー担当 agent/run 識別子: `agent:review-reference-yubikey / run:2026-05-21-yubikey-rr-001`
  - 進捗判定担当: `progress-judge-yubikey-dryrun`
  - 進捗判定担当 agent/run 識別子: `agent:progress-yubikey / run:2026-05-21-yubikey-pj-001`
  - 着手順序: `規約計画 -> 実装計画 -> 規約文書更新 -> 確認 -> レビュー -> 必要時の後続対応`
  - 役割別フォールバック記録（6項目必須）:
    - 対象役割: `参照整合レビュー担当`
    - 起動失敗理由: `専用 reviewer 起動 API が利用不可`
    - 起動失敗証跡: `2026-05-21T14:36+09:00 reviewer launcher error: role not available`
    - 代替実行者: `fallback-reviewer-reference-yubikey`
    - 代替実行者 agent/run 識別子: `agent:review-reference-fallback-yubikey / run:2026-05-21-yubikey-rr-fallback-001`
    - no-reuse 規則充足根拠: `代替実行者は他役割と異なる agent 識別子を使用`
  - 現行対象再読確認: `2026-05-21: 対象コードパス 7 件と対応証跡 2 件を再読済み`
  - 境界注記: `dry-run のためコード差分未作成。確認/レビュー/実装状態は前進させない`
- 次サイクル着手計画スロット（未設定）:
  - 実装担当: `未設定`
  - 確認担当: `未設定`
  - レビュー担当一覧: `未設定`
  - 進捗判定担当: `未設定`
  - 着手順序: `未設定`
- 実装状態: `未実装`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/yubikey.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 完了 | `docs/secret-recovery/yubikey-secret-storage-design.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 確認 | 未着手 | `review-artifacts/yubikey/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未着手 | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手 | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

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
- 次サイクル着手計画スロット（未設定）:
  - 実装担当: `未確定`
  - 実装担当 agent/run 識別子: `未確定`
  - レビュー担当一覧:
    - 構造レビュー担当: `未確定`
    - 構造レビュー担当 agent/run 識別子: `未確定`
    - 運用整合レビュー担当: `未確定`
    - 運用整合レビュー担当 agent/run 識別子: `未確定`
    - セキュリティレビュー担当: `未確定`
    - セキュリティレビュー担当 agent/run 識別子: `未確定`
    - 仕様適合レビュー担当: `未確定`
    - 仕様適合レビュー担当 agent/run 識別子: `未確定`
    - 参照整合レビュー担当: `未確定`
    - 参照整合レビュー担当 agent/run 識別子: `未確定`
  - 進捗判定担当: `未確定`
  - 進捗判定担当 agent/run 識別子: `未確定`
  - 着手順序: `未確定`
  - 役割別フォールバック記録（6項目必須）:
    - 対象役割: `未確定`
    - 起動失敗理由: `未確定`
    - 起動失敗証跡: `未確定`
    - 代替実行者: `未確定`
    - 代替実行者 agent/run 識別子: `未確定`
    - no-reuse 規則充足根拠: `未確定（代替実行者が current orchestrator/current executor ではない根拠を明記）`
  - 現行対象再読確認: `未確定`
  - 境界注記: `未確定`
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
- 次サイクル着手計画スロット（未設定）:
  - 実装担当: `未確定`
  - 実装担当 agent/run 識別子: `未確定`
  - レビュー担当一覧:
    - 構造レビュー担当: `未確定`
    - 構造レビュー担当 agent/run 識別子: `未確定`
    - 運用整合レビュー担当: `未確定`
    - 運用整合レビュー担当 agent/run 識別子: `未確定`
    - セキュリティレビュー担当: `未確定`
    - セキュリティレビュー担当 agent/run 識別子: `未確定`
    - 仕様適合レビュー担当: `未確定`
    - 仕様適合レビュー担当 agent/run 識別子: `未確定`
    - 参照整合レビュー担当: `未確定`
    - 参照整合レビュー担当 agent/run 識別子: `未確定`
  - 進捗判定担当: `未確定`
  - 進捗判定担当 agent/run 識別子: `未確定`
  - 着手順序: `未確定`
  - 役割別フォールバック記録（6項目必須）:
    - 対象役割: `未確定`
    - 起動失敗理由: `未確定`
    - 起動失敗証跡: `未確定`
    - 代替実行者: `未確定`
    - 代替実行者 agent/run 識別子: `未確定`
    - no-reuse 規則充足根拠: `未確定（代替実行者が current orchestrator/current executor ではない根拠を明記）`
  - 現行対象再読確認: `未確定`
  - 境界注記: `未確定`
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
- 次サイクル着手計画スロット（未設定）:
  - 実装担当: `未確定`
  - 実装担当 agent/run 識別子: `未確定`
  - レビュー担当一覧:
    - 構造レビュー担当: `未確定`
    - 構造レビュー担当 agent/run 識別子: `未確定`
    - 運用整合レビュー担当: `未確定`
    - 運用整合レビュー担当 agent/run 識別子: `未確定`
    - セキュリティレビュー担当: `未確定`
    - セキュリティレビュー担当 agent/run 識別子: `未確定`
    - 仕様適合レビュー担当: `未確定`
    - 仕様適合レビュー担当 agent/run 識別子: `未確定`
    - 参照整合レビュー担当: `未確定`
    - 参照整合レビュー担当 agent/run 識別子: `未確定`
  - 進捗判定担当: `未確定`
  - 進捗判定担当 agent/run 識別子: `未確定`
  - 着手順序: `未確定`
  - 役割別フォールバック記録（6項目必須）:
    - 対象役割: `未確定`
    - 起動失敗理由: `未確定`
    - 起動失敗証跡: `未確定`
    - 代替実行者: `未確定`
    - 代替実行者 agent/run 識別子: `未確定`
    - no-reuse 規則充足根拠: `未確定（代替実行者が current orchestrator/current executor ではない根拠を明記）`
  - 現行対象再読確認: `未確定`
  - 境界注記: `未確定`
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
- 次サイクル着手計画スロット（未設定）:
  - 実装担当: `未確定`
  - 実装担当 agent/run 識別子: `未確定`
  - レビュー担当一覧:
    - 構造レビュー担当: `未確定`
    - 構造レビュー担当 agent/run 識別子: `未確定`
    - 運用整合レビュー担当: `未確定`
    - 運用整合レビュー担当 agent/run 識別子: `未確定`
    - セキュリティレビュー担当: `未確定`
    - セキュリティレビュー担当 agent/run 識別子: `未確定`
    - 仕様適合レビュー担当: `未確定`
    - 仕様適合レビュー担当 agent/run 識別子: `未確定`
    - 参照整合レビュー担当: `未確定`
    - 参照整合レビュー担当 agent/run 識別子: `未確定`
  - 進捗判定担当: `未確定`
  - 進捗判定担当 agent/run 識別子: `未確定`
  - 着手順序: `未確定`
  - 役割別フォールバック記録（6項目必須）:
    - 対象役割: `未確定`
    - 起動失敗理由: `未確定`
    - 起動失敗証跡: `未確定`
    - 代替実行者: `未確定`
    - 代替実行者 agent/run 識別子: `未確定`
    - no-reuse 規則充足根拠: `未確定（代替実行者が current orchestrator/current executor ではない根拠を明記）`
  - 現行対象再読確認: `未確定`
  - 境界注記: `未確定`
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
- 次サイクル着手計画スロット（未設定）:
  - 実装担当: `未確定`
  - 実装担当 agent/run 識別子: `未確定`
  - レビュー担当一覧:
    - 構造レビュー担当: `未確定`
    - 構造レビュー担当 agent/run 識別子: `未確定`
    - 運用整合レビュー担当: `未確定`
    - 運用整合レビュー担当 agent/run 識別子: `未確定`
    - セキュリティレビュー担当: `未確定`
    - セキュリティレビュー担当 agent/run 識別子: `未確定`
    - 仕様適合レビュー担当: `未確定`
    - 仕様適合レビュー担当 agent/run 識別子: `未確定`
    - 参照整合レビュー担当: `未確定`
    - 参照整合レビュー担当 agent/run 識別子: `未確定`
  - 進捗判定担当: `未確定`
  - 進捗判定担当 agent/run 識別子: `未確定`
  - 着手順序: `未確定`
  - 役割別フォールバック記録（6項目必須）:
    - 対象役割: `未確定`
    - 起動失敗理由: `未確定`
    - 起動失敗証跡: `未確定`
    - 代替実行者: `未確定`
    - 代替実行者 agent/run 識別子: `未確定`
    - no-reuse 規則充足根拠: `未確定（代替実行者が current orchestrator/current executor ではない根拠を明記）`
  - 現行対象再読確認: `未確定`
  - 境界注記: `未確定`
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

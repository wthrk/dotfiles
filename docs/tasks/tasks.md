# ルートタスク台帳

この文書は repository-wide の root active ledger であり、active work item の選定と委譲先参照の入口に限定する。

## 現在の作業項目

- `Bitwarden Secrets Manager`

## 作業項目一覧

### YubiKey

- 状態: `完了`
- GitHub issue: #12
- 主成果物: `実コード差分`
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/storage_service.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/domain/model.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 作業定義文書: [secret-recovery/work-items/yubikey.md](secret-recovery/work-items/yubikey.md#12-yubikey-秘密情報保存)
- 領域台帳/履歴: [secret-recovery/tasks.md](secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク)

### Bitwarden Secrets Manager

- 状態: `未開始`
- GitHub issue: #13
- 主成果物: `実コード差分`
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/domain/model.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 作業定義文書: [secret-recovery/work-items/bitwarden-secrets-manager.md](secret-recovery/work-items/bitwarden-secrets-manager.md#13-bitwarden-secrets-manager-クライアント)
- レビュー記録: [secret-recovery/review-artifacts/bitwarden-secrets-manager/review.md](secret-recovery/review-artifacts/bitwarden-secrets-manager/review.md)
- 領域台帳/履歴: [secret-recovery/tasks.md](secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク)

### GnuPG / SSH

- 状態: `未開始`
- GitHub issue: #14
- 主成果物: `実コード差分`
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 作業定義文書: [secret-recovery/work-items/gnupg-ssh.md](secret-recovery/work-items/gnupg-ssh.md)
- レビュー記録: [secret-recovery/review-artifacts/gnupg-ssh/review.md](secret-recovery/review-artifacts/gnupg-ssh/review.md)
- 領域台帳/履歴: [secret-recovery/tasks.md](secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク)

### Git

- 状態: `未開始`
- GitHub issue: #15
- 主成果物: `実コード差分`
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 作業定義文書: [secret-recovery/work-items/git.md](secret-recovery/work-items/git.md#15-password-store-復元)
- レビュー記録: [secret-recovery/review-artifacts/git/review.md](secret-recovery/review-artifacts/git/review.md)
- 領域台帳/履歴: [secret-recovery/tasks.md](secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク)

### Bitwarden Password Manager

- 状態: `未開始`
- GitHub issue: #16
- 主成果物: `実コード差分`
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 作業定義文書: [secret-recovery/work-items/bitwarden-password-manager.md](secret-recovery/work-items/bitwarden-password-manager.md#16-bitwarden-password-manager-cli-ログイン)
- レビュー記録: [secret-recovery/review-artifacts/bitwarden-password-manager/review.md](secret-recovery/review-artifacts/bitwarden-password-manager/review.md)
- 領域台帳/履歴: [secret-recovery/tasks.md](secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク)

### 新規マシン復旧フロー統合

- 状態: `未開始`
- GitHub issue: #17
- 主成果物: `実コード差分`
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 作業定義文書: [secret-recovery/work-items/integration.md](secret-recovery/work-items/integration.md#17-新規マシン復旧フロー統合)
- レビュー記録: [secret-recovery/review-artifacts/integration/review.md](secret-recovery/review-artifacts/integration/review.md)
- 領域台帳/履歴: [secret-recovery/tasks.md](secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク)

### ガバナンス文書整合

- 状態: `完了`
- 主成果物: `文書差分`
- 対象文書パス:
  - `.agents/skills/`
  - `AGENTS.md`
  - `AGENTS_ja.md`
  - `docs/secret-recovery/implementation-guidelines.md`
  - `docs/task-governance/`
  - `docs/tasks/`
  - `docs/docs-governance.md`
- 作業定義文書: [repo-governance/work-items/global-documentation-remediation.md](repo-governance/work-items/global-documentation-remediation.md#repo-global-ガバナンス文書整合)
- レビュー記録: [repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md](repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md)
- 現行サイクル確認/レビュー記録（2026-05-22）:
  - [repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md](repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md)
  - [repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md](repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md)
- 履歴レビュー記録（2026-05-21）: [repo-governance/review-artifacts/global-documentation-remediation/review.md](repo-governance/review-artifacts/global-documentation-remediation/review.md)
- 領域台帳/履歴: [repo-governance/tasks.md](repo-governance/tasks.md#repo-global-ガバナンス文書整合タスク)

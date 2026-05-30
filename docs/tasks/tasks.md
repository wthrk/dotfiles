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
  - `rust/dotfiles-cli/src/secrets/application/run_*.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/domain.rs`
  - `rust/dotfiles-cli/src/secrets/domain/manifest.rs`
  - `rust/dotfiles-cli/src/secrets/domain/material.rs`
  - `rust/dotfiles-cli/src/secrets/domain/piv.rs`
  - `rust/dotfiles-cli/src/secrets/domain/storage.rs`
  - `rust/dotfiles-cli/src/secrets/domain/commands.rs`
  - `rust/dotfiles-cli/src/secrets/domain/enrollment.rs`
  - `rust/dotfiles-cli/src/secrets/domain/verification.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`（YubiKey 当時の対象。現行 `11ff088` tree では削除済みであり、現行実装パスとして扱わない）
  - `rust/dotfiles-cli/src/secrets/adapters/io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io/process.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io/report.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey/device_serial_adapter.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey/storage_adapter.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/aead.rs`
  - `rust/dotfiles-cli/src/secrets/support/process_io.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/oaep.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_random.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
  - `rust/dotfiles-cli/Cargo.toml`
- 作業定義文書: [secret-recovery/work-items/yubikey.md](secret-recovery/work-items/yubikey.md#12-yubikey-秘密情報保存)
- レビュー記録: [secret-recovery/review-artifacts/yubikey/review.md](secret-recovery/review-artifacts/yubikey/review.md#yubikey-レビュー記録)
- 現行サイクル確認基準: `2bd7e0a..この実装コメント補正 HEAD current-cycle app regression test and documentation remediation`
- 実装/テスト差分の保存コミット終端: `この実装コメント補正 HEAD`（直前実コード終端 `4c82da8 fix(secrets): protectionテストのsecret assertionを秘匿化` に documentation reviewer Fail の doc comment 補正を加えたもの。自己 hash は本文へ埋め込まず git log の HEAD で確認する）
- 現行補正コミット: `この current-cycle 補正 HEAD`（自己 hash は本文へ埋め込まず、git log の HEAD で確認する）
- 領域台帳/履歴: [secret-recovery/tasks.md](secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク)

### Bitwarden Secrets Manager

- 状態: `進行中（Hypatia 後 fresh review 待ち）`
- GitHub issue: #13
- 主成果物: `実コード差分`
- 対象コードパス:
  - `Cargo.toml`
  - `Cargo.lock`
  - `rust/dotfiles-cli/Cargo.toml`
  - `rust/dotfiles-cli/src/main.rs`
  - `rust/dotfiles-cli/src/lib.rs`
  - `rust/dotfiles-cli/src/cli.rs`
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/entrypoint.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_stdin_json.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_stdin_json.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_get_with.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_stdin.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_stdin.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_setup_with.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/ports/bw.rs`
  - `rust/dotfiles-cli/src/secrets/ports/io.rs`
  - `rust/dotfiles-cli/src/secrets/ports/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/domain.rs`
  - `rust/dotfiles-cli/src/secrets/domain/bws.rs`
  - `rust/dotfiles-cli/src/secrets/domain/commands.rs`
  - `rust/dotfiles-cli/src/secrets/domain/enrollment.rs`
  - `rust/dotfiles-cli/src/secrets/domain/verification.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`（削除）
  - `rust/dotfiles-cli/src/secrets/adapters/bw.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/bw/internal_stub.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io/process.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io/report.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey/device_serial_adapter.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey/selected_device.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey/storage_adapter.rs`
  - `rust/dotfiles-cli/src/secrets/entrypoint/dispatch.rs`
  - `rust/dotfiles-cli/src/secrets/entrypoint/runtime.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/process_io.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/bws.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/piv_pin.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_random.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_consumer.rs`（削除）
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
  - `rust/dotfiles-cli/tests/secrets_internal_stub/mod.rs`
  - `rust/dotfiles-cli/tests/secrets_internal_stub/cli_stub_state.rs`
- 作業定義文書: [secret-recovery/work-items/bitwarden-secrets-manager.md](secret-recovery/work-items/bitwarden-secrets-manager.md#13-bitwarden-secrets-manager-クライアント)
- レビュー記録: [secret-recovery/review-artifacts/bitwarden-secrets-manager/review.md](secret-recovery/review-artifacts/bitwarden-secrets-manager/review.md)
- 現行サイクル差分識別子: `PR #33 / branch refactor/secrets-structure-issue-30-main / base 5ff5e54 / 実装/レビュー対象終端 11ff088 / diff range 5ff5e54..11ff088`
- 履歴サイクル差分識別子: `2026-05-29-hypatia-current-cycle-worktree@HEAD-dccada7`（旧 BSM Hypatia サイクル。PR #33 / Issue #30 現行サイクルの合格根拠として扱わない）
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

### 責務基準レビュー強制への是正

- 状態: `完了`
- 主成果物: `文書差分`
- 対象文書パス:
  - `docs/architecture/review-checklist.md`
  - `.agents/skills/test-review/SKILL.md`
  - `docs/task-governance/implementation-review-judgement.md`
  - `.agents/skills/architectural-consistency-review/SKILL.md`
  - `.agents/skills/orchestration/SKILL.md`
  - `.agents/skills/dotfiles-task-governance/SKILL.md`
  - `.agents/skills/implementation-review-judgement/SKILL.md`
  - `docs/tasks/repo-governance/work-items/responsibility-based-review-enforcement.md`
- 作業定義文書: [repo-governance/work-items/responsibility-based-review-enforcement.md](repo-governance/work-items/responsibility-based-review-enforcement.md#責務基準レビュー強制への是正)
- 確認記録: [repo-governance/review-artifacts/responsibility-based-review-enforcement/confirmation.md](repo-governance/review-artifacts/responsibility-based-review-enforcement/confirmation.md)
- レビュー記録: [repo-governance/review-artifacts/responsibility-based-review-enforcement/review.md](repo-governance/review-artifacts/responsibility-based-review-enforcement/review.md)（集約後レビュー判定: 合格。個別判定: [運用整合](repo-governance/review-artifacts/responsibility-based-review-enforcement/review-operational-2026-05-25.md)・[参照整合](repo-governance/review-artifacts/responsibility-based-review-enforcement/review-reference-2026-05-25.md)）
- 領域台帳/履歴: [repo-governance/tasks.md](repo-governance/tasks.md#repo-global-ガバナンス文書整合タスク)

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

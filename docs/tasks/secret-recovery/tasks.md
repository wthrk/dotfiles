# 新規マシン秘密情報復旧基盤タスク

この文書は secret-recovery の進捗台帳である。進め方は [../../task-governance/workflow.md](../../task-governance/workflow.md#タスク運用ワークフロー)、固定実装単位は [../../secret-recovery/implementation-guidelines.md](../../secret-recovery/implementation-guidelines.md#計画依頼の固定実装単位) を参照する。

## 作業項目一覧

### YubiKey

- 状態: `完了`
- 現行サイクル状態: `完了`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/yubikey.md](work-items/yubikey.md#12-yubikey-秘密情報保存)
- レビュー記録: [review-artifacts/yubikey/review.md](review-artifacts/yubikey/review.md#yubikey-レビュー記録)（現行サイクル状態: 完了）
- 現行サイクル確認基準: `2bd7e0a..この実装コメント補正 HEAD current-cycle app regression test and documentation remediation`
- 実装/テスト差分の保存コミット終端: `この実装コメント補正 HEAD`（直前実コード終端 `4c82da8 fix(secrets): protectionテストのsecret assertionを秘匿化` に documentation reviewer Fail の doc comment 補正を加えたもの。自己 hash は本文へ埋め込まず git log の HEAD で確認する）
- 現行補正コミット: `この current-cycle 補正 HEAD`（自己 hash は本文へ埋め込まず、git log の HEAD で確認する）
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_*.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/domain.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`（YubiKey 当時の対象。現行 `11ff088` tree では削除済みであり、現行実装パスとして扱わない）
  - `rust/dotfiles-cli/src/secrets/adapters/io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io/process.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io/report.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey/device_serial_adapter.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey/storage_adapter.rs`
  - `rust/dotfiles-cli/src/secrets/domain/manifest.rs`
  - `rust/dotfiles-cli/src/secrets/domain/material.rs`
  - `rust/dotfiles-cli/src/secrets/domain/piv.rs`
  - `rust/dotfiles-cli/src/secrets/domain/storage.rs`
  - `rust/dotfiles-cli/src/secrets/domain/commands.rs`
  - `rust/dotfiles-cli/src/secrets/domain/enrollment.rs`
  - `rust/dotfiles-cli/src/secrets/domain/verification.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
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
- 実装状態: `完了`
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
| 確認 | 完了 | `review-artifacts/yubikey/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 完了 | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 完了（不要） | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### Bitwarden Secrets Manager

- 状態: `進行中（Hypatia 後 fresh review 待ち）`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/bitwarden-secrets-manager.md](work-items/bitwarden-secrets-manager.md#13-bitwarden-secrets-manager-クライアント)
- レビュー記録: [review-artifacts/bitwarden-secrets-manager/review.md](review-artifacts/bitwarden-secrets-manager/review.md#bitwarden-secrets-manager-レビュー記録)
- 現行サイクル差分識別子: `PR #33 / branch refactor/secrets-structure-issue-30-main / base 5ff5e54 / 実装/レビュー対象終端 77dc03c / diff range 5ff5e54..77dc03c`
- 履歴サイクル差分識別子: `2026-05-29-hypatia-current-cycle-worktree@HEAD-dccada7`（旧 BSM Hypatia サイクル。PR #33 / Issue #30 現行サイクルの合格根拠として扱わない）
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `Cargo.toml`
  - `Cargo.lock`
  - `rust/dotfiles-cli/Cargo.toml`
  - `rust/dotfiles-cli/src/main.rs`
  - `rust/dotfiles-cli/src/lib.rs`
  - `rust/dotfiles-cli/src/secrets_internal_test_stub_contract.rs`
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
  - `rust/dotfiles-cli/src/secrets/domain/values.rs`（削除）
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
- 実装状態: `Hypatia 後 fresh review 待ち`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `../../secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/bitwarden-secrets-manager.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 完了 | `../../secret-recovery/bitwarden-secrets-manager-design.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 実装（PR #33 / Issue #30） | fresh review 待ち | 実コード差分（`PR #33 / branch refactor/secrets-structure-issue-30-main / base 5ff5e54 / 実装/レビュー対象終端 77dc03c / diff range 5ff5e54..77dc03c`。`11ff088` は直前 P1 対応 commit、`77dc03c` は fresh review 差し戻し（構造・PTY・追跡更新）対応 commit。`ae1b917` は PR #33 証跡同期、`97748c4` は BSM 対象コードパス漏れ指摘への対応、`5e21afb` と `4cd47d4` は PR #33 現行 HEAD 証跡補正、`f2f2f20` は削除済み adapter root を現行対象パス扱いしない台帳補正、`4092a86` は PR #33 差分終端補正） | [work-items/bitwarden-secrets-manager.md](work-items/bitwarden-secrets-manager.md) |
| 実装（BSM 契約: verify/check + fingerprint + recipient + recoverability + stale overwrite） | 未完了（後続 Rust 実装で追跡） | 実コード差分＋テスト | [work-items/bitwarden-secrets-manager.md#13-bitwarden-secrets-manager-クライアント](work-items/bitwarden-secrets-manager.md#13-bitwarden-secrets-manager-クライアント) |
| 確認 | fresh review 前確認済み | `review-artifacts/bitwarden-secrets-manager/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未実施（Hypatia 後 fresh review 必須） | `review-artifacts/bitwarden-secrets-manager/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手（fresh review 合格後に判定） | `review-artifacts/bitwarden-secrets-manager/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

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
| 実装（Design PR 契約の Rust 反映: restore/export/provisioning） | 未着手 | 実コード差分＋テスト | [work-items/gnupg-ssh.md](work-items/gnupg-ssh.md) |
| 確認 | 未着手 | `review-artifacts/gnupg-ssh/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未着手 | `review-artifacts/gnupg-ssh/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手 | `review-artifacts/gnupg-ssh/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### Git

- 状態: `実装済み（現行サイクル集約レビュー合格）`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/git.md](work-items/git.md#15-password-store-復元)
- レビュー記録: [review-artifacts/git/review.md](review-artifacts/git/review.md#git-レビュー記録)（集約後レビュー判定: 合格）
- 現行サイクル差分識別子: `a188d3d..35232c8`（branch `feat/secrets-restore-pass-issue-15`。base `a188d3d` は #14 統合後、HEAD `35232c8`）
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 実装状態: `実装済み（現行サイクル集約レビュー合格）`
- 確認・レビュー: 確認通過（`cargo build`/`test`(186 unit + 33 stub integration)/`clippy -D warnings`/`fmt --check`/`xtask check`、restore-gpg(#14) 退行なし）。必須7担当全員合格・集約後レビュー判定 `合格`（`review-artifacts/git/review.md`）。
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/git.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 進行中 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 実装（restore-pass: spec L174 手順の Rust 反映） | 実装済み（集約レビュー合格） | 実コード差分＋テスト（`a188d3d..35232c8`） | [work-items/git.md](work-items/git.md#15-password-store-復元) |
| 確認 | 完了 | `review-artifacts/git/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 完了（集約後レビュー判定: 合格） | `review-artifacts/git/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 完了（不要・未解消 finding なし） | `review-artifacts/git/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### Bitwarden Password Manager

- 状態: `実装済み（現行サイクル集約レビュー合格）`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/bitwarden-password-manager.md](work-items/bitwarden-password-manager.md#16-bitwarden-password-manager-cli-ログイン)
- レビュー記録: [review-artifacts/bitwarden-password-manager/review.md](review-artifacts/bitwarden-password-manager/review.md#bitwarden-password-manager-レビュー記録)（集約後レビュー判定: 合格）
- 後続義務（#17 委譲・本 #16 実装で解消）: #17（PR #42）が stub 契約検証に留めた `verify-yubikey --check bw-login` を、本 #16 実装（PR #43）が実際の `bw login` / `bw unlock` 到達確認として実装し、#17 で記録された CLI 起動可能性確認の既知制約を解消済み。詳細と強制条件は作業定義文書の構造完了条件・レビュー合格条件を正本とする。
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 実装状態: `実装済み（現行サイクル集約レビュー合格）`
- 現行サイクル差分識別子: `branch feat/secrets-bw-login-issue-16 / base 1318b19 / diff range main..feat/secrets-bw-login-issue-16`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/bitwarden-password-manager.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 進行中 | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 実装（bw-login: spec L176-178 手順の Rust 反映 + verify-yubikey `--check bw-login` 実体化 + README 文書化） | 実装済み（集約レビュー合格） | 実コード差分＋テスト（`main..feat/secrets-bw-login-issue-16`） | [work-items/bitwarden-password-manager.md](work-items/bitwarden-password-manager.md#16-bitwarden-password-manager-cli-ログイン) |
| 確認 | 完了 | `review-artifacts/bitwarden-password-manager/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 完了（集約後レビュー判定: 合格） | `review-artifacts/bitwarden-password-manager/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 完了（構造指摘是正済み・未解消 finding なし） | `review-artifacts/bitwarden-password-manager/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

### 新規マシン復旧フロー統合

- 状態: `実装済み（現行サイクル集約レビュー合格）`
- 主成果物: `実コード差分`
- 作業定義文書: [work-items/integration.md](work-items/integration.md#17-新規マシン復旧フロー統合)
- レビュー記録: [review-artifacts/integration/review.md](review-artifacts/integration/review.md)（集約後レビュー判定: 合格）
- 現行サイクル差分識別子: `1318b19..db34795`（base origin/main `1318b19`、最終終端 `db34795`。branch `feat/secrets-recovery-flow-integration-issue-17`。サイクル1 初版 `bdb3171` を PR #42 AI レビュー P1 検出で差し戻し、サイクル2 `b0340d7`・サイクル3 `991245d`・サイクル4 `db34795` の remediation を経て最終集約合格）
- 現行サイクル確認/レビュー記録（2026-06-03）:
  - [review-artifacts/integration/confirmation.md](review-artifacts/integration/confirmation.md)
  - [review-artifacts/integration/review.md](review-artifacts/integration/review.md)
- 粗粒度進捗: [issue-11-progress.md](issue-11-progress.md#11-系粗粒度進捗)
- 対象コードパス:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_bw_login.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs`
  - `rust/dotfiles-cli/src/secrets/domain.rs`
  - `rust/dotfiles-cli/src/secrets/domain/bw_login.rs`
  - `rust/dotfiles-cli/src/secrets/domain/commands.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/ports/bw_login.rs`
  - `rust/dotfiles-cli/src/secrets/ports/io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/bw_login.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/bw_login/internal_stub.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io/process.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/io/report.rs`
  - `rust/dotfiles-cli/src/secrets/entrypoint/dispatch.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/bw_login.rs`
  - `rust/dotfiles-cli/src/secrets_internal_test_stub_contract.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
- 実装状態: `実装済み（現行サイクル集約レビュー合格）`
- 確認・レビュー: 確認通過（`cargo build`(default + `secrets-internal-test-stub`)/`clippy -D warnings`/`fmt --check`/`test`(209 unit + 42 stub integration)/`xtask check`、既存コマンド退行なし、stdout 漏洩防止テスト追加）。実装差分必須7担当 + 文書是正の参照整合レビュー担当 全員合格・集約後レビュー判定 `合格`（`review-artifacts/integration/review.md`、最終対象差分 `1318b19..db34795`。サイクル1〜4の差し戻し経緯を含む）。
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `work-items/integration.md` | [implementation-guidelines.md#規約計画](../../secret-recovery/implementation-guidelines.md#規約計画) |
| 実装計画 | 完了 | `work-items/integration.md` | [implementation-guidelines.md#実装計画](../../secret-recovery/implementation-guidelines.md#実装計画) |
| 規約文書更新 | 完了（spec に `--check bw-login` 現行範囲＝CLI 起動可能性確認＋#16 委譲を恒久仕様化） | `docs/secret-recovery/secret-recovery-spec.md` | [implementation-guidelines.md#規約文書更新](../../secret-recovery/implementation-guidelines.md#規約文書更新) |
| 実装（bw-login 結線 + verify-yubikey --check bw-login） | 実装済み（集約レビュー合格） | 実コード差分＋テスト（`1318b19..db34795`） | [work-items/integration.md](work-items/integration.md#17-新規マシン復旧フロー統合) |
| 確認 | 完了 | `review-artifacts/integration/confirmation.md` | [implementation-guidelines.md#確認](../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 完了（集約後レビュー判定: 合格） | `review-artifacts/integration/review.md` | [implementation-guidelines.md#レビュー](../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 完了（不要・未解消 finding なし） | `review-artifacts/integration/review.md` | [implementation-guidelines.md#必要時の後続対応](../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

## 移管履歴

- `2026-05-21`: `最終ドキュメント整理` は secret-recovery の通常作業項目から除外し、repo-global の文書整合作業として [../repo-governance/tasks.md](../repo-governance/tasks.md#ガバナンス文書整合) へ移管した。関連する台帳・作業定義・確認/レビュー証跡の正本は repo-governance 側を参照する。

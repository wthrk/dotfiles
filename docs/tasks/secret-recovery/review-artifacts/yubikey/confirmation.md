# YubiKey 確認記録

この文書は `docs/tasks/secret-recovery/work-items/yubikey.md` の現行サイクル確認証跡（current worktree 基準）である。

## 現行サイクル（2026-05-27）

- 確認状態: `実施済み（再レビュー待ち）`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 対象差分識別子: `yubikey-current-cycle-2026-05-27-6fd4014-ddf027e`
- 確認基準: `ddf027e docs(secrets): YubiKey参照基準を6fd4014へ同期`
- 保存コミット列:
  - `9352e14 refactor(secrets): yubikey実機IOをport実装へ内包`
  - `e1a0a0a refactor(secrets): piv adapter補助ファイルを内包`
  - `1cc9889 refactor(secrets): 保護secret操作をprotection内部へ閉じる`
  - `cc39c6b docs(secrets): YubiKey運用証跡を9352e14基準へ同期`
  - `ce7dc31 refactor(secrets): secret入力規則をdomainへ寄せる`
  - `01979bf docs(secrets): YubiKey現行サイクル参照を同期`
  - `6fd4014 refactor(secrets): storage復元規則をdomainへ移す`
  - `ddf027e docs(secrets): YubiKey参照基準を6fd4014へ同期`
- `6fd4014..ddf027e` の変更ファイル集合:
  - `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-reference-agents-minimal-2026-05-26.md`
  - `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-reference-agents-overview-2026-05-26.md`
  - `docs/tasks/repo-governance/review-artifacts/responsibility-based-review-enforcement/review-reference-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/confirmation.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review-doc-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review-operational-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review-test-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review.md`
  - `docs/tasks/secret-recovery/tasks.md`
  - `docs/tasks/secret-recovery/work-items/yubikey.md`
  - `docs/tasks/tasks.md`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_stdin_json.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_stdin_json.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_get_with.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_stdin.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
- 対象スコープ:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_*.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/domain.rs`
  - `rust/dotfiles-cli/src/secrets/domain/manifest.rs`
  - `rust/dotfiles-cli/src/secrets/domain/material.rs`
  - `rust/dotfiles-cli/src/secrets/domain/piv.rs`
  - `rust/dotfiles-cli/src/secrets/domain/storage.rs`
  - `rust/dotfiles-cli/src/secrets/domain/values.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/aead.rs`
  - `rust/dotfiles-cli/src/secrets/support/process_io.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/oaep.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_consumer.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_random.rs`
  - `rust/dotfiles-cli/src/secrets/support/version.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
  - `rust/dotfiles-cli/Cargo.toml`

## 実行コマンド

- `direnv exec . cargo check -p dotfiles-cli`
- `direnv exec . cargo xtask check`
- `direnv exec . cargo clippy --workspace --all-targets`
- `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`

## 結果要約

- `cargo check`: 実行済み
- `cargo test --no-run`: 実行済み
- `cargo xtask check`: 実行済み（`8740b1a`）
- `cargo clippy --workspace --all-targets`: 実行済み（`8740b1a`）
- `RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 実行済み（`8740b1a`）
- 判定: `再レビュー待ち`

## 2026-05-26 ad92152 基準履歴追記

- 履歴基準コミット: `ad92152 refactor(secrets): align yubikey storage boundaries`
- 追加保存コミット: `f6d5d7c fix(secrets): keep pin secret access inside protection`
- 追加保存コミット: `022c21b fix(secrets): resolve yubikey review blockers`
- 確認証跡同期コミット: `8740b1a docs(secrets): sync yubikey current cycle commit`
- 確認証跡同期コミット: `734823d docs(secrets): record yubikey verification evidence`
- 追加保存コミット: `e148c0d fix(secrets): YubiKey再レビュー指摘を修正`
- 追加保存コミット: `e06bf4d fix(secrets): adapter公開面をport実装型へ限定`
- 追加保存コミット: `41084ae fix(secrets): adapter境界のclippy指摘を修正`
- 追加保存コミット: `78f10ac refactor(secrets): object逆引き規則をdomainへ移管`
- 追加保存コミット: `9ff38d7 refactor(secrets): 上書き可否規則をdomainへ移管`
- 現行状態: `再レビュー待ち`
- 確認前提: セキュリティレビューは `判定: 合格` 済み。構造レビュー / 運用整合レビューの `判定: 要修正` 指摘は本追記以降の保存点で反映済みであり、集約合格とは記録しない。
- `ad92152` 以降の変更ファイル集合:
  - `docs/task-governance/workflow.md`
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `docs/tasks/secret-recovery/tasks.md`
  - `docs/tasks/secret-recovery/work-items/yubikey.md`
  - `docs/secret-recovery/secret-recovery-spec.md`
  - `docs/secret-recovery/yubikey-secret-storage-design.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/confirmation.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review.md`

## 2026-05-27 e148c0d 検証追記

- 対象コミット: `e148c0d fix(secrets): YubiKey再レビュー指摘を修正`
- `direnv exec . cargo xtask check`: 成功
- `direnv exec . cargo clippy --workspace --all-targets`: 成功
- `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 成功
- 判定: `再レビュー待ち`

## 2026-05-27 41084ae 検証追記

- 対象コミット: `41084ae fix(secrets): adapter境界のclippy指摘を修正`
- `cargo check -p dotfiles-cli`: 成功
- `git diff --check`: 成功
- `direnv exec . cargo xtask check`: 成功
- `direnv exec . cargo clippy --workspace --all-targets`: 成功
- `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 成功
- 判定: `再レビュー待ち`

## 2026-05-27 78f10ac 検証追記

- 対象コミット: `78f10ac refactor(secrets): object逆引き規則をdomainへ移管`
- 修正内容: PIV object ID から `SecretName` への逆引き規則を adapter stub から `domain::piv::SecretName::from_object_id` へ移した。
- `cargo check -p dotfiles-cli`: 成功
- `git diff --check`: 成功
- 判定: `再レビュー待ち`

## 2026-05-27 9ff38d7 検証追記

- 対象コミット: `9ff38d7 refactor(secrets): 上書き可否規則をdomainへ移管`
- 修正内容: `put` の既存 secret 上書き可否判定を `domain::piv::SecretName::ensure_write_allowed` へ移した。
- `cargo check -p dotfiles-cli`: 成功
- `git diff --check`: 成功
- 判定: `再レビュー待ち`

## 2026-05-27 adapter 構造説明同期

- 対象コミット: `959269a docs(secrets): adapter構造修正の検証証跡を追記`
- 確認内容: current-cycle の structural 修正説明を実コードの現構成へ同期した。
- 現構成:
  - entrypoint は `SecretsAdapters::default()` を利用する。
  - `adapters::real_secrets_boundary()` は存在しない。
  - `JsonReportAdapter` と公開 constructor は存在しない。
  - report 翻訳は `ReportPort for RealSecretsBoundary` の trait 実装境界に閉じている。
- 判定: `再レビュー待ち`

## 2026-05-27 9352e14 基準 operational 履歴追記

- 履歴サイクル識別子: `yubikey-history-2026-05-27-base-9352e14`
- 履歴基準コミット: `9352e14 refactor(secrets): yubikey実機IOをport実装へ内包`
- 追加保存コミット: `e1a0a0a refactor(secrets): piv adapter補助ファイルを内包`
- 追加保存コミット: `1cc9889 refactor(secrets): 保護secret操作をprotection内部へ閉じる`
- 追加保存コミット: `cc39c6b docs(secrets): YubiKey運用証跡を9352e14基準へ同期`
- 本 operational 修正文書コミット: `01979bf docs(secrets): YubiKey現行サイクル参照を同期`
- レビュー前保存コミット扱い: 上記保存コミットは作業状態を失わないための中間保存点であり、レビュー合格、完了判定、または `S3 -> S4` の commit gate 充足根拠にはしない。
- management key 前提: 現行 YubiKey work item サイクルでは factory-default management key を暫定前提にする。非既定 management key への切替、取得、注入は次フェーズの鍵管理作業で扱う。これは完了判定上の既知例外であり、リスクは次フェーズで閉じる。
- `9352e14..01979bf` の変更ファイル集合:
  - `docs/secret-recovery/secret-recovery-spec.md`
  - `docs/secret-recovery/yubikey-secret-storage-design.md`
  - `docs/task-governance/implementation-review-judgement.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/confirmation.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/structural-review.md`
  - `docs/tasks/secret-recovery/tasks.md`
  - `docs/tasks/secret-recovery/work-items/yubikey.md`
  - `docs/tasks/tasks.md`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/report.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/secret_io.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/domain/piv.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/process_io.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/oaep.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_consumer.rs`
- 確認結果:
  - `9352e14..01979bf` の実差分出力と上記変更ファイル集合を同期した。
  - `cc39c6b` と `01979bf` を保存コミット列へ明示した。
  - 本追記では operational 証跡整合のみを扱い、合格とは記録しない。

## 2026-05-26 追加実装サイクル追記

- 未解決 1,2,3,4,5,6,7,9 のコード是正を反映した。
- 未解決 10 は本追記を含め review/confirmation/work-item の同期を再実施した。
- 判定前提更新: same-route 維持を前提に、`--test-stub-yubikey` / `yubikey_runtime` / 別 binary / 別 CLI / command-scenario branching / port-boundary swap は採用しない。

## ブロッカー要約

- review artifact 間で判定の整合が崩れていたため、レビュー判定は `要修正` を維持し、task/current-cycle 状態は `再レビュー待ち` として同期した。
- 履歴レビュー内に現行 code path と不一致な参照が混在していたため、現行実在パスへ更新した。

## 前進可否

- 前進可否: `前進不可（差し戻し継続）`
- 理由: `review.md` の現行サイクル集約判定は再レビュー前の `要修正` を維持しているため。

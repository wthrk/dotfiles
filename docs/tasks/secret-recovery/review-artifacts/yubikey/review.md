# YubiKey レビュー記録

この文書は `docs/tasks/secret-recovery/work-items/yubikey.md` の現行サイクル集約レビュー正本である。

## 現行サイクル（2026-05-27）

- 集約後レビュー判定: `未確定（再レビュー待ち）`
- 集約判定要約: current-cycle artifact が未生成の担当について、状態を `未実施（再レビュー待ち）` / `判定: 未確定` に統一した。現行サイクルは reviewer 再実施待ちのため、集約判定は確定しない。
- 対象差分識別子: `yubikey-current-cycle-2026-05-27-6fd4014-095ab3b`
- 対象ブランチ: `feat/yubikey-secret-storage`
- current-cycle reviewer 判定追跡（2026-05-28時点）:
  - `structural`: artifact path `未生成` / 状態 `未実施（再レビュー待ち）` / 判定 `未確定`
  - `operational`: artifact path `未生成` / 状態 `未実施（再レビュー待ち）` / 判定 `未確定`
  - `security`: artifact path `未生成` / 状態 `未実施（再レビュー待ち）` / 判定 `未確定`
  - `specification-conformance`: artifact path `未生成` / 状態 `未実施（再レビュー待ち）` / 判定 `未確定`
  - `test`: artifact path `未生成` / 状態 `未実施（再レビュー待ち）` / 判定 `未確定`
  - `documentation`: artifact path `未生成` / 状態 `未実施（再レビュー待ち）` / 判定 `未確定`
  - `architectural-consistency`: artifact path `未生成` / 状態 `未実施（再レビュー待ち）` / 判定 `未確定`
  - `reference-integrity`（文書修正別枠）: artifact path `未生成` / 状態 `未実施（再レビュー待ち）` / 判定 `未確定`
- 保存コミット列:
  - `9352e14 refactor(secrets): yubikey実機IOをport実装へ内包`
  - `e1a0a0a refactor(secrets): piv adapter補助ファイルを内包`
  - `1cc9889 refactor(secrets): 保護secret操作をprotection内部へ閉じる`
  - `cc39c6b docs(secrets): YubiKey運用証跡を9352e14基準へ同期`
  - `ce7dc31 refactor(secrets): secret入力規則をdomainへ寄せる`
  - `01979bf docs(secrets): YubiKey現行サイクル参照を同期`
  - `6fd4014 refactor(secrets): storage復元規則をdomainへ移す`
  - `90178f0 refactor(secrets): protection文言を汎用化する`
  - `52dac47 refactor(secrets): PIN検証をdomain適用へ戻す`
  - `a9f510e refactor(secrets): bootstrap文書規則をdomain側へ戻す`
  - `ddf027e docs(secrets): YubiKey参照基準を6fd4014へ同期`
  - `d5f9ca9 refactor(secrets): secret値制約をdomainへ戻す`
  - `cd68ab4 docs(secrets): YubiKey現行サイクルをddf027eへ同期`
  - `821accc refactor(secrets): setup前提判定をdomainへ移す`
  - `36b5311 docs(secrets): 削除済み参照を履歴注記へ分離`
  - `ac36952 refactor(secrets): 読み出し値制約をdomainへ戻す`
  - `234d64a refactor(secrets): sealed blobを汎用supportへ寄せる`
  - `ee7dfc6 refactor(secrets): 復号失敗の意味づけをdomain側へ戻す`
  - `1906050 test(secrets): sealed blobの境界検証を追加`
  - `e164160 refactor(secrets): sealed blobをpayload境界へ中立化`
  - `8b4d0fe docs(secrets): support層コメントを中立化`
  - `ca3d78c style(secrets): sealed blobを整形`
  - `8df2209 refactor(secrets): secret materialをdomain opaque化`
  - `917cff4 refactor(secrets): secret material backend境界を縮小`
  - `7e88a81 docs(secrets): YubiKey現行サイクルを917cff4へ同期`
  - `7facae0 test(secrets): storage intent domain規則を検証`
  - `913d857 test(secrets): device route internal検証を復旧`
  - `c819fc0 test(secrets): mockito internal stubをfeature注入する`
  - `b2871d8 test(secrets): usecase stubをmockitoで復旧する`
  - `7bae68d test(secrets): usecaseテスト名とケースを復旧する`
  - `1e770a0 test(secrets): real-route監査とwrite-event検証を復旧`
  - `e619fba test(secrets): application internal testsを復旧`
  - `850eb54 test(secrets): app usecaseテストをmockitoで復旧`
  - `5217c7f fix(secrets): appテストのmockito依存をfeature有効時に限定`
  - `7744b0b fix(secrets): current-cycle証跡とinternal test経路を同期`
  - `91e9fed fix(secrets): operational fail証跡を補正`
  - `095ab3b docs(secrets): current-cycle reviewer整合とinternal test証跡を是正`

### 2026-05-28 current-cycle 証跡是正コミット

- 是正コミット: `7744b0b fix(secrets): current-cycle証跡とinternal test経路を同期`
- 是正コミット: `91e9fed fix(secrets): operational fail証跡を補正`
- 是正コミット: `095ab3b docs(secrets): current-cycle reviewer整合とinternal test証跡を是正`
- 紐付け: latest HEAD `095ab3b` を current-cycle 記録の保存コミット終端として扱う。
- `6fd4014..5217c7f` の変更ファイル集合:
  - `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-reference-agents-minimal-2026-05-26.md`
  - `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-reference-agents-overview-2026-05-26.md`
  - `docs/tasks/repo-governance/review-artifacts/responsibility-based-review-enforcement/confirmation.md`
  - `docs/tasks/repo-governance/review-artifacts/responsibility-based-review-enforcement/review-reference-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/confirmation.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review-doc-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review-operational-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review-spec-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review-test-2026-05-25.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review.md`
  - `docs/tasks/secret-recovery/tasks.md`
  - `docs/tasks/secret-recovery/work-items/yubikey.md`
  - `docs/tasks/tasks.md`
  - `Cargo.toml`
  - `Cargo.lock`
  - `rust/tests/checks/src/static_checks.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_stdin_json.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_stdin_json.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_get_with.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_stdin.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_prompt.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_stdin.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_setup_with.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs`
  - `rust/dotfiles-cli/src/secrets/domain/material.rs`
  - `rust/dotfiles-cli/src/secrets/domain/piv.rs`
  - `rust/dotfiles-cli/src/secrets/domain/storage.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
  - `rust/dotfiles-cli/Cargo.toml`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
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

### 2026-05-26 ad92152 基準履歴追記

- 履歴基準コミット: `ad92152 refactor(secrets): align yubikey storage boundaries`
- 追加保存コミット: `f6d5d7c fix(secrets): keep pin secret access inside protection`
- 追加保存コミット: `022c21b fix(secrets): resolve yubikey review blockers`
- 確認証跡同期コミット: `734823d docs(secrets): record yubikey verification evidence`
- 追加保存コミット: `e148c0d fix(secrets): YubiKey再レビュー指摘を修正`
- 追加保存コミット: `e06bf4d fix(secrets): adapter公開面をport実装型へ限定`
- 追加保存コミット: `41084ae fix(secrets): adapter境界のclippy指摘を修正`
- 追加保存コミット: `78f10ac refactor(secrets): object逆引き規則をdomainへ移管`
- 追加保存コミット: `9ff38d7 refactor(secrets): 上書き可否規則をdomainへ移管`
- 現行状態: `再レビュー待ち`
- レビュー前提: この節は履歴サイクル（2026-05-26）専用記録であり、現行サイクル（2026-05-27）の reviewer 判定には使用しない。
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
- structural 差し戻し修正:
  - `adapters.rs` の再エクスポート集約と公開 factory を廃止し、runtime adapter は port trait 実装型 `SecretsAdapters` として entrypoint へ渡す。
  - 旧 adapter 補助ファイルの route label helper を公開 helper から private trait 実装へ閉じる。
  - 旧 report adapter 生成を廃止し、route は現行の port trait 実装境界で使う。
- 追加 structural 差し戻し修正:
  - `adapters.rs` の `piv_io` module 公開を廃止し、entrypoint は `SecretsAdapters::default()` だけを利用する。
  - `JsonReportAdapter` とその constructor を廃止し、report 翻訳は `ReportPort for RealSecretsBoundary` の trait 実装境界へ閉じる。
- operational 差し戻し修正:
  - 履歴サイクルの基準を `ad92152` として明示し、変更ファイル集合を記録した。
  - `tasks.md` と `work-items/yubikey.md` を `再レビュー待ち` / `修正済み` 基準で同期した。
  - 保存コミット規定は既存コミットを書き換えず、現行サイクル追記として運用実態と整合させる。
- 追加 operational 差し戻し修正:
  - レビュー前保存コミットは `S3 -> S4` の終端コミットとは別の中間保存点であり、レビュー合格/完了コミット gate を満たさないことを workflow 正本へ明記した。
  - 今後の保存点コミットメッセージを `<type>(<scope>): <日本語説明>` に統一することを workflow 正本へ明記した。
- e148c0d 検証結果:
  - `direnv exec . cargo xtask check`: 成功
  - `direnv exec . cargo clippy --workspace --all-targets`: 成功
  - `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 成功
- 41084ae 検証結果:
  - `cargo check -p dotfiles-cli`: 成功
  - `git diff --check`: 成功
  - `direnv exec . cargo xtask check`: 成功
  - `direnv exec . cargo clippy --workspace --all-targets`: 成功
  - `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 成功
- 78f10ac 修正内容:
  - PIV object ID から `SecretName` への逆引き規則を adapter stub から `domain::piv::SecretName::from_object_id` へ移した。
- 78f10ac 検証結果:
  - `cargo check -p dotfiles-cli`: 成功
  - `git diff --check`: 成功
- 9ff38d7 修正内容:
  - `put` の既存 secret 上書き可否判定を `domain::piv::SecretName::ensure_write_allowed` へ移した。
- 9ff38d7 検証結果:
  - `cargo check -p dotfiles-cli`: 成功
  - `git diff --check`: 成功

### 2026-05-27 9352e14 基準 operational 履歴追記

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
  - 履歴注記: 旧補助ファイル群は現行 tree では削除済みで、現行参照 path は `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`。
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

### 2026-05-26 追加実装サイクル追記

- 解消済み（本サイクル）: 未解決 1,2,3,4,5,6,7,9
- 継続（次サイクル）: 未解決 10（最終集約判定の再更新）
- 判定前提更新: `adapters` 配下にあること単独では違反根拠にせず、配置と責務を合わせて評価する。same-route 維持（別 binary / 別 CLI / command-scenario branching / port-boundary swap 禁止）を継続条件とする。

### 履歴レビュー結果（現行サイクル判定対象外）

- `review-security-2026-05-25.md`、`review-operational-2026-05-25.md`、`review-spec-2026-05-25.md`、`review-test-2026-05-25.md`、`review-doc-2026-05-25.md`、`structural-review.md` は履歴サイクル用 artifact として保持する。
- 上記 artifact の判定結果は current-cycle（2026-05-27）の reviewer 状態を前進させる根拠として使用しない。

## 役割別レビュー

### 構造レビュー担当

- 判定: `未確定`
- 判定要約: current-cycle artifact 未生成のため、判定を確定しない（再レビュー待ち）。
- 根拠:
  - work item の差し戻し状態に合わせ、完了判定へ前進させない。

### 運用整合レビュー担当

- 判定: `未確定`
- 判定要約: current-cycle artifact 未生成のため、判定を確定しない（再レビュー待ち）。
- 根拠:
  - `confirmation.md` と同一 diff identifier に統一済み。

### セキュリティレビュー担当

- 判定: `未確定`
- 判定要約: current-cycle artifact 未生成のため、判定を確定しない（再レビュー待ち）。
- 根拠:
  - `review-security-2026-05-25.md` は履歴専用であり、現行サイクルの判定根拠に使わない。

### 仕様適合レビュー担当

- 判定: `未確定`
- 判定要約: current-cycle artifact 未生成のため、判定を確定しない（再レビュー待ち）。
- 根拠:
  - `docs/tasks/secret-recovery/work-items/yubikey.md` の差し戻し前提に従う。

### テストレビュー担当

- 判定: `未確定`
- 判定要約: current-cycle artifact 未生成のため、判定を確定しない（再レビュー待ち）。
- 根拠:
  - 本更新はレビュー証跡整合の是正であり、テスト観点の新規完了判定は未実施。

### ドキュメントレビュー担当

- 判定: `未確定`
- 判定要約: current-cycle artifact 未生成のため、判定を確定しない（再レビュー待ち）。
- 根拠:
  - `application/run_*.rs` の公開 entrypoint と core workflow 非自明 helper に対する doc comment coverage 欠落を合格扱いにできない運用へ是正した。
  - `ports`/`adapters`/`support` の層責務境界を担う非自明要素で、`why`/責任分界説明の欠落を blocker 扱いに統一した。

### アーキテクチャ整合レビュー担当

- 判定: `未確定`
- 判定要約: current-cycle artifact 未生成のため、判定を確定しない（再レビュー待ち）。
- 根拠:
  - work item の差し戻しサイクルに整合させる。

## 集約

- 集約後レビュー判定: `未確定（再レビュー待ち）`
- 集約判定要約: required reviewers の current-cycle artifact が未生成のため、判定確定を行わない。
- 集約根拠:
  - `confirmation.md` と `review.md` の diff identifier を一致させた。
  - stale file path 参照を現行存在パスへ更新した。
  - 矛盾する「未実施/要修正」混在を `未実施（再レビュー待ち）` / `判定: 未確定` に統一した。

# YubiKey レビュー記録

この文書は `docs/tasks/secret-recovery/work-items/yubikey.md` の現行サイクル集約レビュー正本である。

## 現行サイクル（2026-05-26）

- 集約後レビュー判定: `要修正`
- 集約判定要約: `ad92152` 基準の structural / operational 差し戻し修正を反映した。合格判定は未実施のため、現行サイクルは再レビュー待ちとして扱う。
- 対象差分識別子: `yubikey-current-cycle-2026-05-26-base-ad92152`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 対象スコープ:
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_*.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/domain.rs`
  - `rust/dotfiles-cli/src/secrets/domain/model.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/aead.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/oaep.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`

### 2026-05-26 ad92152 基準 current-cycle 追記

- current-cycle 基準コミット: `ad92152 refactor(secrets): align yubikey storage boundaries`
- 追加保存コミット: `f6d5d7c fix(secrets): keep pin secret access inside protection`
- 追加保存コミット: `022c21b fix(secrets): resolve yubikey review blockers`
- 確認証跡同期コミット: `734823d docs(secrets): record yubikey verification evidence`
- 追加保存コミット: `e148c0d fix(secrets): YubiKey再レビュー指摘を修正`
- 追加保存コミット: `e06bf4d fix(secrets): adapter公開面をport実装型へ限定`
- 追加保存コミット: `41084ae fix(secrets): adapter境界のclippy指摘を修正`
- 追加保存コミット: `78f10ac refactor(secrets): object逆引き規則をdomainへ移管`
- 追加保存コミット: `9ff38d7 refactor(secrets): 上書き可否規則をdomainへ移管`
- 現行状態: `再レビュー待ち`
- レビュー前提: security は pass 済み。structural / operational の Fail 指摘は本追記以降の保存点で修正済みとして扱い、合格とは記録しない。
- `ad92152` 以降の変更ファイル集合:
  - `docs/task-governance/workflow.md`
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/yubikey_pin.rs`
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
  - current-cycle の基準を `ad92152` として明示し、変更ファイル集合を記録した。
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

### 2026-05-27 9352e14 基準 operational 追記

- current-cycle 追加識別子: `yubikey-current-cycle-2026-05-27-base-9352e14`
- current-cycle 基準コミット: `9352e14 refactor(secrets): yubikey実機IOをport実装へ内包`
- 追加保存コミット: `e1a0a0a refactor(secrets): piv adapter補助ファイルを内包`
- 追加保存コミット: `1cc9889 refactor(secrets): 保護secret操作をprotection内部へ閉じる`
- 本 operational 修正文書コミット: `この追記を含む保存コミット`
- レビュー前保存コミット扱い: 上記保存コミットは作業状態を失わないための中間保存点であり、レビュー合格、完了判定、または `S3 -> S4` の commit gate 充足根拠にはしない。
- management key 前提: 現行 YubiKey work item サイクルでは factory-default management key を暫定前提にする。非既定 management key への切替、取得、注入は次フェーズの鍵管理作業で扱う。これは完了判定上の既知例外であり、リスクは次フェーズで閉じる。
- `9352e14` 以降の変更ファイル集合:
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/confirmation.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review.md`
  - `docs/tasks/secret-recovery/tasks.md`
  - `docs/tasks/secret-recovery/work-items/yubikey.md`
  - `docs/secret-recovery/secret-recovery-spec.md`
  - `docs/secret-recovery/yubikey-secret-storage-design.md`
  - `docs/tasks/tasks.md`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/report.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/secret_io.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/process_io.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/oaep.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_consumer.rs`
- 確認結果:
  - `9352e14` 保存点の後続で adapter 補助ファイル内包と protection 内部化の保存コミットを確認した。
  - 本追記では operational 証跡整合のみを扱い、合格とは記録しない。

### 2026-05-26 追加実装サイクル追記

- 解消済み（本サイクル）: 未解決 1,2,3,4,5,6,7,9
- 継続（次サイクル）: 未解決 10（最終集約判定の再更新）
- 判定前提更新: `adapters` 配下にあること単独では違反根拠にせず、配置と責務を合わせて評価する。same-route 維持（別 binary / 別 CLI / command-scenario branching / port-boundary swap 禁止）を継続条件とする。

## 役割別レビュー

### 構造レビュー担当

- 判定: `要修正`
- 判定要約: `application/run_*.rs` と `adapters/piv_io.rs` 系の責務境界について、現行サイクルでの再判定を継続する。
- 根拠:
  - work item の差し戻し状態に合わせ、完了判定へ前進させない。

### 運用整合レビュー担当

- 判定: `要修正`
- 判定要約: 現行サイクル差分識別子を統一したが、サイクル状態は差し戻し継続。
- 根拠:
  - `confirmation.md` と同一 diff identifier に統一済み。

### セキュリティレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 本スコープ更新は review artifact 整合修正であり、新規の秘密情報露出差分なし。

### 仕様適合レビュー担当

- 判定: `要修正`
- 判定要約: work item の現行サイクル状態 (`再レビュー待ち`) と整合するため、完了判定を保留。
- 根拠:
  - `docs/tasks/secret-recovery/work-items/yubikey.md` の差し戻し前提に従う。

### テストレビュー担当

- 判定: `要修正`
- 判定要約: 現行サイクルを差し戻し継続として扱うため、合格固定を行わない。
- 根拠:
  - 本更新はレビュー証跡整合の是正であり、テスト観点の新規完了判定は未実施。

### ドキュメントレビュー担当

- 判定: `要修正`
- 判定要約: `application/run_*.rs` を中心に必須 doc comment coverage を blocker 条件として再評価する必要がある。
- 根拠:
  - `application/run_*.rs` の公開 entrypoint と core workflow 非自明 helper に対する doc comment coverage 欠落を合格扱いにできない運用へ是正した。
  - `ports`/`adapters`/`support` の層責務境界を担う非自明要素で、`why`/責任分界説明の欠落を blocker 扱いに統一した。

### アーキテクチャ整合レビュー担当

- 判定: `要修正`
- 判定要約: 現行サイクル完了判定を行うには追加の再評価が必要。
- 根拠:
  - work item の差し戻しサイクルに整合させる。

## 集約

- 集約後レビュー判定: `要修正`
- 集約判定要約: 現行サイクルは差し戻し継続。review artifact の不整合は是正したが、完了ゲートは未充足。
- 集約根拠:
  - `confirmation.md` と `review.md` の diff identifier を一致させた。
  - stale file path 参照を現行存在パスへ更新した。
  - 矛盾する「合格/要修正」混在を現行サイクル判定に統一した。

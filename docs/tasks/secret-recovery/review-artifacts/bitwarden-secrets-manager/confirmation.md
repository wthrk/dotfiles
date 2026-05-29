# Bitwarden Secrets Manager 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Secrets Manager` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `完了（レビュー合格・commit前）`
- 判定位置づけ: `デザインPR段階 current-cycle 差分の確認完了（作業項目全体の完了判定ではない）`
- 対象差分識別子: `bws-design-pr-current-cycle`
- 対象ブランチ: `copilot/bitwarden-secrets-manager-client`
- 確認開始時点参照: `../../work-items/bitwarden-secrets-manager.md` 記載の `実装/テスト差分の保存コミット終端`
- 差分区分: `実装`

## 確認手順と結果

- 手順:
  - `direnv exec . cargo fmt -p dotfiles-cli`
  - `direnv exec . cargo check -p dotfiles-cli`
  - `direnv exec . cargo check -p dotfiles-cli --features secrets-internal-test-stub`
  - `direnv exec . cargo test -p dotfiles-cli --lib secrets::adapters::bws_client::tests`
  - `direnv exec . cargo test -p dotfiles-cli --lib secrets::application::run_verify_yubikey_with::tests::verify_executes_bws_external_check_when_requested`
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli`
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application::run_verify_yubikey_with::tests::verify_executes_bws_external_check_when_requested`
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application`
  - `git diff --check`
- 結果:
  - `cargo fmt -p dotfiles-cli` 成功
  - `cargo check -p dotfiles-cli` 成功
  - `cargo check -p dotfiles-cli --features secrets-internal-test-stub` 成功
  - `bws_client` の adapter 境界 unit test 成功（secret key mapping / default 構築 / unprotected token backend rejection）
  - `verify_executes_bws_external_check_when_requested` passed（`BwsClientPort` 経由の BWS check 実行経路を application test で確認）
  - `secrets-internal-test-stub --test secrets_cli verify_yubikey_runs_bws_external_check` は過去記録。production module への feature `include!` 差し替えを除去したため、この named CLI integration test は現行差分では存在しない。
  - `secrets-internal-test-stub --test secrets_cli` 成功（0 tests。production binary を feature で test double へ差し替えないことを確認）
  - `secrets-internal-test-stub --lib ...verify_executes_bws_external_check_when_requested` 成功（feature 有効時も BWS external check の application 経路が port 契約で通ることを確認）
  - `secrets-internal-test-stub --lib secrets::application` 成功（63 passed。`application.rs` の test-only bridge が internal feature 有効時だけ compile されることを確認）
  - `git diff --check` 成功
- 未実施理由（未実施がある場合）: `なし`

## 実装進捗への影響

- 対象コードパス差分:
  - `rust/dotfiles-cli/src/secrets/domain/values.rs` — `BwsSecretName`、`RestoreGpgCommand`、`RestorePassCommand` 追加
  - `rust/dotfiles-cli/src/secrets/ports.rs` — `BwsClientPort` trait 追加
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs` — BWS check 実装（`BwsClientPort` 経由でトークン読み出し＋両 BWS secret fetch）
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_prompt.rs` — `secrets-internal-test-stub` application bridge の cfg 追従
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_stdin_json.rs` — `secrets-internal-test-stub` application bridge の cfg 追従
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_prompt.rs` — `secrets-internal-test-stub` application bridge の cfg 追従
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_stdin_json.rs` — `secrets-internal-test-stub` application bridge の cfg 追従
  - `rust/dotfiles-cli/src/secrets/application/run_get_with.rs` — `secrets-internal-test-stub` application bridge の cfg 追従
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_prompt.rs` — `secrets-internal-test-stub` application bridge の cfg 追従
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_stdin.rs` — `secrets-internal-test-stub` application bridge の cfg 追従
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_prompt.rs` — BWS token rotate prompt 経路
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_stdin.rs` — BWS token rotate stdin 経路
  - `rust/dotfiles-cli/src/secrets/application/run_setup_with.rs` — `secrets-internal-test-stub` application bridge の cfg 追従
  - `rust/dotfiles-cli/src/main.rs` — async CLI entrypoint
  - `rust/dotfiles-cli/src/lib.rs` — async library dispatch 境界
  - `rust/dotfiles-cli/src/cli.rs` — async secrets dispatch 呼び出し
  - `rust/dotfiles-cli/src/secrets/application.rs` — 新規 module 宣言追加
  - `rust/dotfiles-cli/src/secrets.rs` — BWS 関連 command ルーティングと `BwsClientPort` bound 追加
  - `rust/dotfiles-cli/src/secrets/adapters.rs` — `BwsClientAdapter` フィールド、`BwsClientPort` impl 追加
  - `rust/dotfiles-cli/src/secrets/adapters/bws_client.rs` — Bitwarden SDK crate/API adapter 実装
  - `rust/dotfiles-cli/src/secrets/adapters/bws_client_real.rs` — `real` suffix production module 削除
  - `rust/dotfiles-cli/src/secrets/adapters/bws_client_stub.rs` — production source tree 内 stub module 削除
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` — `piv_io` の責務分割後の共通境界定義へ再構成
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/device_selection.rs` — device selection 旧 module 削除
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/device_serial_adapter.rs` — device serial port 翻訳責務を分離
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/process_io_adapter.rs` — process I/O port 翻訳責務を分離
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/storage_adapter.rs` — storage port 翻訳責務を分離
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/report_adapter.rs` — JSON report 変換責務を分離
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/selected_device_real.rs` — `real` suffix production module 削除
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/selected_device_stub.rs` — production source tree 内 stub module 削除
  - `rust/dotfiles-cli/src/secrets/support.rs` — secret support / protection backend 境界の module 宣言
  - `rust/dotfiles-cli/src/secrets/support/process_io.rs` — process-generic I/O helper
  - `rust/dotfiles-cli/src/secrets/support/protection.rs` — core dump 抑止、protected buffer、protection backend module 宣言
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs` — protected input buffer
  - `rust/dotfiles-cli/src/secrets/support/protection/bws.rs` — BWS SDK が secret を必要とする処理を protection backend 境界内で完了する操作
  - `rust/dotfiles-cli/src/secrets/support/protection/piv_pin.rs` — PIV PIN verification の protection 境界
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_random.rs` — secret random / OAEP encrypt helper
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs` — sealed blob helper
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_consumer.rs` — 汎用 plaintext consumer API module 削除
  - `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs` — `BwsClientPort` mock impl 追加
  - `rust/dotfiles-cli/tests/secrets_cli.rs` — BWS CLI 経路と feature build 確認対象
  - `rust/dotfiles-cli/tests/secrets_internal_stub/piv_io_internal_stub.rs` — internal feature test support
  - `rust/dotfiles-cli/Cargo.toml` — Bitwarden SDK / tokio / internal feature dependency
  - `Cargo.toml` / `Cargo.lock` — workspace dependency resolution
- 文書整合メモ: `docs/tasks/secret-recovery/tasks.md` の固定実装単位トラッカー `確認` と本記録の状態は `完了` で同期する。
- 前進可否メモ: BWS external check 相当の application 経路と feature build/test は確認済み。必須レビュー担当の個別判定と集約判定は `review.md` で合格済み。GitHub comment / resolve / commit / push は未実施。

## セキュリティ確認結果

- 秘密値/認証情報の露出確認: `完了` — `BwsClientAdapter` は `ProtectedSecret` backend 確認後、BWS SDK login request を `support/protection` 内の BWS 専用操作で作成・zeroize する。request buffer は Drop で zeroize する guard に保持し、await 後の明示呼び出しへ依存しない。BWS SDK 返却 `String` は `protect_secret_value` へ渡った直後に `Zeroizing<String>` へ移し、その後に `SecretSession::start()` と `ProtectedSecret` 確保を行う。secret 値をログ/エラー本文へ出力しない方針は維持する。
- ログ/引数/一時ファイル/stdout/stderr 確認: `完了` — SDK 呼び出し失敗時の user-visible error は固定要約のみを返し、secret 値や raw API 応答本文を埋め込まない。
- 権限境界/永続化/失敗時挙動確認: `完了` — 通常ビルドの `BwsClientAdapter` は SDK 経路のみを持つ。token は `ProtectedSecret` 借用境界内で処理し、SDK が所有 plaintext buffer の move を要求する login request は `support/protection` 内の BWS 専用操作で呼び出し直前にだけ作る。永続化なし。

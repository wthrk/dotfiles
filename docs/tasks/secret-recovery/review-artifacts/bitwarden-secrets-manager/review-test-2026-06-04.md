# Bitwarden Secrets Manager テストレビュー（2026-06-04）

判定: 合格
判定要約: 所見なし
根拠:
- 対象: current worktree（branch `fix/bws-provisioning-inputs-issue-44`、未コミット変更を含む）で、`docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md` の完了条件のうちテストで検証すべき項目を分類して確認した。
- テストで検証すべき項目:
  - `verify-yubikey --check bws` が `gpg-secret-key-backup` envelope schema を検証すること。
  - `metadata.primary_fingerprint` が lowercase hex 40 文字、separator なしであることを検証すること。
  - 接続中 YubiKey recipient matching（`yubikey_serial` と `public_key_fingerprint`）を検証すること。
  - unwrap-free recoverability check を検証すること。
  - BWS secret の取得成功だけで完了扱いにせず、`password-store-remote` 取得と妥当性確認まで通すこと。
  - spare recipient 追加を含む `gpg-secret-key-backup` 更新 path と `password-store-remote` provisioning/update path で stale overwrite prevention を通すこと。
  - CLI/provisioning 入力経路が、BWS access token を hidden prompt / stdin から取り、`password-store-remote` clone URL を `--url` / 可視 prompt / stdin から取ること。
  - integration test が internal stub の内部遷移ではなく、初期 datastore 定義と CLI 実行後の stdout sentinel 最終観測を検証すること。
- 構造確認・文書確認で満たす項目として分類し、テスト網羅不足にしない項目:
  - SDK 呼び出しの adapter / port 境界隔離、secret 保護境界、support への業務判断混入有無、application/domain/entrypoint/adapters の責務分離、domain の SDK/I/O 非依存は構造・仕様・セキュリティ側の確認対象であり、テスト担当としてはテスト double 配置とテスト責務逸脱の有無だけを確認した。
  - 必須レビュー担当集合、集約判定、PR review comment 運用、補助記録同期は運用・完了判定側の確認対象であり、テスト網羅要求の対象外とした。
- BWS check の envelope/schema/fingerprint/recipient/recoverability/password-store-remote:
  - `rust/dotfiles-cli/tests/secrets_cli.rs` の `verify_yubikey_runs_bws_external_check` は production CLI binary を `verify-yubikey --serial 2001 --check bws` で実行し、`bws` check が `ok` になり、stdout sentinel の final BWS observation で `gpg-secret-key-backup` と `password-store-remote` の両方が解決されていることを確認している。
  - 同ファイルの `verify_yubikey_bws_check_reports_failed_for_invalid_backup_schema`、`verify_yubikey_bws_check_reports_failed_for_invalid_primary_fingerprint`、`verify_yubikey_bws_check_reports_failed_for_recipient_mismatch`、`verify_yubikey_bws_check_reports_failed_when_recoverability_is_not_established` は CLI 経路で失敗 status を確認している。
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs` の unit tests は、`bws_check_fails_when_gpg_backup_schema_is_invalid`、`bws_check_fails_when_primary_fingerprint_is_not_lowercase_hex_40`、`bws_check_fails_when_connected_yubikey_recipient_does_not_match`、`bws_check_fails_when_unwrap_free_recoverability_cannot_be_established`、`verify_bws_check_fetches_required_secrets_and_reports_ok` で、BWS lookup、envelope parse failure、fingerprint failure、connected recipient failure、unwrap-free recoverability failure、`password-store-remote` fetch までの use case 条件を直接検証している。
- stale overwrite prevention:
  - `rust/dotfiles-cli/src/secrets/application/run_add_gpg_backup_spare.rs` の `add_spare_updates_with_guard_after_confirmation` は spare recipient 追加後の `update_gpg_backup_envelope_if_unchanged` に取得済み `BackupUpdateGuard` が渡ることを確認し、`add_spare_stops_when_guard_mismatch_blocks_update` は `BackupUpdateGuard::ensure_matches` 由来の mismatch error で停止することを確認している。
  - `rust/dotfiles-cli/src/secrets/application/run_provision_password_store_remote.rs` の `provision_updates_with_guard_after_confirmation` は `fetch_password_store_remote_guard` 後に確認、URL 入力、`update_password_store_remote_if_unchanged` が guard 付きで呼ばれることを確認し、`provision_stops_when_guard_mismatch_blocks_update` は guard mismatch が provisioning 全体を停止させることを確認している。
  - `rust/dotfiles-cli/src/secrets/adapters/bw/internal_stub.rs` は update 直前に現行値から `BackupUpdateGuard::from_value_bytes` を作り、`expected_guard.ensure_matches(&current_guard)` 後にだけ更新する。integration test 側はこの backend 内部 schema や遷移 helper を持たず、最終 observation だけを読むため、test 側責務は逸脱していない。
- CLI/provisioning 入力経路:
  - `rust/dotfiles-cli/src/secrets/entrypoint/dispatch.rs` は `pass-remote register` を `run_provision_password_store_remote` へ接続し、`ProcessIoAdapter` を BWS token input、BWS client、URL input、overwrite confirmation として渡している。
  - `rust/dotfiles-cli/src/secrets/adapters/io/process.rs` は BWS access token を terminal では hidden prompt、非 terminal では stdin line として読み、`password-store-remote` は terminal では visible prompt、非 terminal では plain stdin line として読む。
  - `rust/dotfiles-cli/tests/secrets_cli.rs` の `pass_remote_register_overwrites_existing_secret_with_tty_confirmation`、`pass_remote_register_overwrites_existing_secret_from_url_argument_with_yes`、`pass_remote_register_overwrites_existing_secret_via_stdin_pipe_with_yes`、`pass_remote_register_stops_non_interactive_overwrite_without_yes`、`pass_remote_register_stops_when_input_url_is_invalid` は、TTY prompt、`--url`、stdin pipe、非対話 confirmation failure、不正 URL failure を production CLI binary 経由で検証している。
  - `rust/dotfiles-cli/src/secrets/application/run_provision_password_store_remote.rs` の unit tests は、BWS access token が provisioning 専用 input port から取得されること、YubiKey storage port を使わないこと、`--url` 指定時に URL input port を呼ばないこと、URL input port 経由の値を domain validation 後に create/update することを mockall で確認している。
- script 静的 grep の混入確認:
  - `rg -n "script|grep|bash -n|provision-secret-recovery-source|scripts/provision|static" rust/dotfiles-cli/tests/secrets_cli.rs rust/dotfiles-cli/src/secrets/application/run_provision_password_store_remote.rs -S` を確認し、Rust CLI integration test に script 静的 grep や `scripts/provision-secret-recovery-source.sh` の静的検査を入れていない。hit は `bootstrap_json` helper と `material` helper のみで、script/static grep ではない。
- test double / fixture 配置:
  - `rust/dotfiles-cli/tests/secrets_cli.rs` は `#![cfg(feature = "secrets-internal-test-stub")]` の CLI integration test で、adapter stub module を import せず `CARGO_BIN_EXE_dotfiles` を実行している。
  - test 側の `StubPorts` は port ごとの初期 spec JSON を env に渡すだけで、BWS/YubiKey/GPG/Git/BW login の backend state schema・状態遷移 helper・write event helper・bincode schema・共有 state file を保持していない。final assertion は stdout sentinel observation から port ごとの最終状態を読む。
  - BWS と YubiKey の stub spec は別 env、別 adapter stub、別 observation port であり、共通巨大 `StubState` や共有 state file で結合していない。
- 実行確認:
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli -- --nocapture`: 49 tests passed。
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib -- --nocapture`: 197 tests passed。

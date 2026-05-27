# YubiKey 確認記録

この文書は `docs/tasks/secret-recovery/work-items/yubikey.md` の現行サイクル確認証跡（current worktree 基準）である。

## 現行サイクル（2026-05-27）

- 確認状態: `実施済み（再レビュー待ち）`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 対象差分識別子: `yubikey-current-cycle-2026-05-28-implementation-2bd7e0a-4c82da8-plus-documentation-comment-remediation-head`
- 確認基準: `2bd7e0a..この実装コメント補正 HEAD current-cycle app regression test and documentation remediation`
- 実装/テスト差分の保存コミット終端: `この実装コメント補正 HEAD`（直前実コード終端 `4c82da8 fix(secrets): protectionテストのsecret assertionを秘匿化` に documentation reviewer Fail の doc comment 補正を加えたもの。自己 hash は本文へ埋め込まず git log の HEAD で確認する）
- 現行補正コミット: `この current-cycle 補正 HEAD`。この commit 自身の hash は本文へ埋め込まず、git log の HEAD で確認する。
- current-cycle reviewer 判定追跡（2026-05-28時点）:
  - `structural`: 状態 `再レビュー待ち` / 判定 `未実施（この実装コメント補正 HEAD 追加差分対象。過去 Pass の持ち越しでは閉じない）`
  - `operational`: 状態 `再レビュー待ち` / 判定 `未実施（本記録修正後の current-cycle 補正 HEAD 対象）`
  - `security`: 状態 `実施済み` / 判定 `合格（4e00605 対象）`
  - `specification-conformance`: 状態 `再レビュー待ち` / 判定 `未実施（この実装コメント補正 HEAD 追加差分対象。過去 Pass の持ち越しでは閉じない）`
  - `test`: 状態 `再レビュー待ち` / 判定 `未実施（この実装コメント補正 HEAD 追加差分対象。過去 Pass の持ち越しでは閉じない）`
  - `documentation`: 状態 `再レビュー待ち` / 判定 `未実施（documentation Fail の doc comment 補正 HEAD 対象。過去 Pass の持ち越しでは閉じない）`
  - `architectural-consistency`: 状態 `再レビュー待ち` / 判定 `未実施（この実装コメント補正 HEAD 追加差分対象。過去 Pass の持ち越しでは閉じない）`
  - `reference-integrity`: 状態 `再レビュー待ち` / 判定 `未実施（本記録修正後の current-cycle 補正 HEAD 対象）`
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
  - `c1cc107 docs(secrets): operational fail記録をcurrent-cycle未確定へ統一`
  - `1b47dd2 docs(secrets): current-cycle判定ラベルと差分基準を是正`
  - `9a87f46 fix(secrets): internal stub seamを単一路adapterへ統一`
  - `bbef5d4 fix(secrets): V15のstub実装をpiv_io本体から分離`
  - `2bd7e0a fix(secrets): internal stub helper公開を除去`
  - `d301035 fix(secrets): YubiKey直近レビューFailを解消`
  - `0f37005 test(secrets): appテストdoubleをmockitoへ集約`
  - `a910eca fix(secrets): YubiKeyレビューFailを再修正`
  - `02281d2 test(secrets): 履歴上のapp回帰テストを復旧`
  - `b0c5fd5 fix(secrets): secret assertion outputを秘匿化`
  - `4c82da8 fix(secrets): protectionテストのsecret assertionを秘匿化`
  - `この実装コメント補正 HEAD`（documentation reviewer Fail の doc comment 補正 commit。自己 hash は本文へ埋め込まず git log で確認する）
  - `この current-cycle 補正 HEAD`（実装コメント補正と証跡整合を含む commit。自己 hash は本文へ埋め込まず git log で確認する）

## 2026-05-28 app 回帰テスト復旧追記

- 実装コミット: `02281d2 test(secrets): 履歴上のapp回帰テストを復旧`
- 履歴確認対象:
  - `092cbbc:rust/dotfiles-cli/src/secrets/application.rs`
  - `092cbbc:rust/dotfiles-cli/src/secrets/application/storage_service_tests.rs`
- 復旧対象:
  - `rotate_bws_token_rejects_already_updated_serial`: `rust/dotfiles-cli/src/secrets/application.rs`
  - `partial_rotate_summary_serializes_updated_entries`: `rust/dotfiles-cli/src/secrets/domain/values.rs`
  - `partial_rotate_summary_skips_output_when_empty`: `rust/dotfiles-cli/src/secrets/domain/values.rs`
  - `put_rejects_noninteractive_without_stdin_option`: `rust/dotfiles-cli/src/secrets/application.rs`
  - `setup_stops_when_management_auth_precondition_fails`: `rust/dotfiles-cli/src/secrets/application.rs`
  - `setup_uses_management_auth_for_precondition_and_manifest_write`: `rust/dotfiles-cli/src/secrets/application.rs`
  - `put_get_and_verify_round_trip_through_device`: `rust/dotfiles-cli/src/secrets/application.rs`
  - `put_uses_management_auth_for_each_secret_write`: `rust/dotfiles-cli/src/secrets/application.rs`
  - `rotate_bws_token_preserves_other_secrets`: `rust/dotfiles-cli/src/secrets/application.rs`
  - `rotate_uses_management_auth_for_token_replacement`: `rust/dotfiles-cli/src/secrets/application.rs`
  - `decryption_fails_when_blob_is_replayed_to_different_serial`: `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
  - `decryption_fails_when_secret_blob_name_and_object_are_swapped`: `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
- mockito 共通 support: `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs` を拡張し、store/load/output を mockito route 経由の共有 state に集約した。app/usecase test 用の独自 fake/stub は追加していない。
- 現行責務への写像: 旧 storage-service の manifest/storage policy は domain/support の inline unit test へ、usecase の実依存代替は app mockito support へ配置した。management key は現フェーズ default management key 前提のまま、旧 management-auth 系 test は「setup/store port 境界が precondition/write を呼ぶこと」の回帰として復旧した。
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application -- --list`: 成功（43 tests）
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application`: 成功（43 passed, 0 failed）
- `direnv exec . cargo test -p dotfiles-cli secrets::application:: --lib`: 成功（0 passed, 40 filtered out）
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli`: 成功（22 passed, 0 failed）
- `direnv exec . cargo check -p dotfiles-cli`: 成功
- `direnv exec . cargo clippy -p dotfiles-cli --all-targets`: 成功
- `git diff --check`: 成功
- 状態: `再レビュー待ち`

## 2026-05-28 security Fail 修正追記

- 実装コミット: `b0c5fd5 fix(secrets): secret assertion outputを秘匿化`
- 修正対象:
  - `rust/dotfiles-cli/src/secrets/application.rs`: `output_secret_value()` / `stored_secret_value(...)` の平文 `assert_eq!` を長さ + SHA-256 digest 比較へ置換し、失敗時 output に secret/load bytes を出さない。
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`: 復号 plaintext の平文 `assert_eq!` を長さ + SHA-256 digest 比較へ置換し、失敗時 output に plaintext bytes を出さない。
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application`: 成功（43 passed, 0 failed）
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::`: 成功（80 passed, 0 failed）
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli`: 成功（22 passed, 0 failed）
- `direnv exec . cargo check -p dotfiles-cli`: 成功
- `direnv exec . cargo clippy -p dotfiles-cli --all-targets`: 成功
- `git diff --check`: 成功
- 状態: `security 再レビュー待ち`

## 2026-05-28 security Fail 追加修正追記

- 実装コミット: `4c82da8 fix(secrets): protectionテストのsecret assertionを秘匿化`
- 修正対象:
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs`: `with_secret` 内の平文 `assert_eq!` を長さ + SHA-256 digest 比較へ置換し、失敗時 output に secret bytes を出さない。
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`: `ProtectedSecret::try_clone` 系 test の平文 `assert_eq!` を長さ + SHA-256 digest 比較へ置換し、失敗時 output に ProtectedSecret plaintext bytes を出さない。
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application`: 成功（43 passed, 0 failed）
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::`: 成功（80 passed, 0 failed）
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli`: 成功（22 passed, 0 failed）
- `direnv exec . cargo check -p dotfiles-cli`: 成功
- `direnv exec . cargo clippy -p dotfiles-cli --all-targets`: 成功
- `git diff --check`: 成功
- 状態: `security / operational / reference-integrity 再レビュー待ち`

## 2026-05-28 documentation Fail コメント補正追記

- 実装コメント補正コミット: `この実装コメント補正 HEAD`（自己 hash は本文へ埋め込まず、git log の HEAD で確認する）
- 修正対象:
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`: file-level comment と、`SealRequest` / `SealWithKeyWrapRequest` / `SealedBlob` / `seal` / `seal_with_key_wrap` / `open_with_key_unwrap` の doc comment を追加し、AEAD・key wrap・AAD・payload id の責務境界を明記した。
  - `rust/dotfiles-cli/src/secrets/support/aead.rs`: `aes_256_gcm_from_key` / `encrypt_detached` / `decrypt_detached` の doc comment を追加し、nonce/tag/AAD の扱いと caller responsibility を明記した。
  - `rust/dotfiles-cli/src/secrets/support/protection/oaep.rs`: `mgf1_sha256` / `xor_with_mask` / `find_oaep_separator` の doc comment を追加し、OAEP 復元の失敗条件、padding 判定、secret buffer 境界を明記した。
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_random.rs`: `rsa_oaep_encrypt` の doc comment を補正し、public key 境界、`ProtectedSecret` 借用中だけの平文化、opaque wrapped key 返却、失敗時に未 wrap key material を露出しない責務を明記した。
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_random.rs`: `random_secret` の doc comment を補正し、`rand` の process-local CSPRNG source、生成先が locked `ProtectedSecret` であること、lock 確保失敗時に raw buffer を返さないことを明記した。
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`: `ProtectedSecret::try_clone` の doc comment を補正し、唯一許可される copy 経路、copy 前 lock、失敗時に unlocked copy を返さないこと、caller が直接 copy 経路を作ってはいけない理由を明記した。
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`: `decode_json_string_map` の doc comment を補正し、secret JSON bytes の借用中 parse、平文 `String` の生存範囲、field limit 失敗条件、返却値が locked `ProtectedSecret` map に閉じることを明記した。
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`: `protect_locked_secret_value` の doc comment を補正し、raw `Vec<u8>` と `LockGuard` が同一 allocation に対応する caller responsibility、`Zeroizing` 管理へ入る境界、interrupt 時に返さず Drop/zeroize へ進む失敗契約を明記した。
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs`: `into_trimmed_bytes_and_lock` の doc comment を追加し、raw `Vec<u8>` と `LockGuard` への一時分離、zeroize 管理の一時的な外れ、失敗条件、caller が直後に `ProtectedSecret` へ移す責務を明記した。
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`: `open_decoded` の doc comment を追加し、content key / AAD / tag による検証済み復号境界、認証失敗時に plaintext として返さない失敗契約、返却値が `ProtectedSecret` に閉じることを明記した。
- 挙動変更: なし。doc comment / file-level comment の追加のみ。
- `direnv exec . cargo fmt --check`: 成功
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::support::`: 成功（19 passed, 0 failed, 64 filtered out）
- `git diff --check`: 成功
- 状態: `documentation 再レビュー待ち`

## 2026-05-28 current-cycle 証跡是正コミット

- 是正コミット: `7744b0b fix(secrets): current-cycle証跡とinternal test経路を同期`
- 是正コミット: `91e9fed fix(secrets): operational fail証跡を補正`
- 是正コミット: `095ab3b docs(secrets): current-cycle reviewer整合とinternal test証跡を是正`
- 是正コミット: `c1cc107 docs(secrets): operational fail記録をcurrent-cycle未確定へ統一`
- 是正コミット: `1b47dd2 docs(secrets): current-cycle判定ラベルと差分基準を是正`
- 実装コミット: `9a87f46 fix(secrets): internal stub seamを単一路adapterへ統一`
- 追加実装コミット: `bbef5d4 fix(secrets): V15のstub実装をpiv_io本体から分離`
- 追加実装コミット: `2bd7e0a fix(secrets): internal stub helper公開を除去`
- 追加修正コミット: `d301035 fix(secrets): YubiKey直近レビューFailを解消`
- 追加修正コミット: `0f37005 test(secrets): appテストdoubleをmockitoへ集約`
- 追加修正コミット: `a910eca fix(secrets): YubiKeyレビューFailを再修正`
- 追加修正コミット: `02281d2 test(secrets): 履歴上のapp回帰テストを復旧`
- 追加修正コミット: `b0c5fd5 fix(secrets): secret assertion outputを秘匿化`
- 追加修正コミット: `4c82da8 fix(secrets): protectionテストのsecret assertionを秘匿化`
- 追加修正コミット: `この実装コメント補正 HEAD`（documentation reviewer Fail の doc comment 補正。自己 hash は本文へ埋め込まず git log で確認する）
- 現行補正コミット: `この current-cycle 補正 HEAD`（自己 hash は本文へ埋め込まず、git log の HEAD で確認する）
- 追加修正: `2bd7e0a..この実装コメント補正 HEAD` の current-cycle Fail remediation。`a910eca` は structural/documentation の bridge 誤判定対策、internal stub helper private 化、app mockito response body 非露出化を含み、`02281d2` は git 履歴上の旧 app/storage-service 回帰テスト復旧を含み、`b0c5fd5` と `4c82da8` は security Fail の assertion output 秘匿化を含み、この実装コメント補正 HEAD は documentation Fail の support 暗号/安全境界 doc comment 補正を含む。
- 紐付け: 実装/テスト差分の保存コミット終端は `この実装コメント補正 HEAD`。この current-cycle 補正 HEAD は support 暗号/安全境界 doc comment 補正と review artifact / ledger 整合を同じ current-cycle 補正として保持する。補正 commit 自身の hash は自己参照固定点にならないため本文へ埋め込まない。`2bd7e0a..この実装コメント補正 HEAD` は直近レビュー Fail、app 回帰テスト未復旧 Fail、security Fail、documentation Fail を対象とする。`eab7e66 docs(secrets): YubiKey運用証跡を38d3a09へ固定` は `38d3a09` 基準の過去履歴 commit であり、`4c82da8` 後の current-cycle 証跡同期コミットとして扱わない。
- 実装差分集合: `6fd4014..この実装コメント補正 HEAD` の変更ファイル集合:
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
  - `docs/architecture/hexagonal-implementation-rules.md`
  - `docs/architecture/review-checklist.md`
  - `.agents/skills/structural-review/SKILL.md`
  - `Cargo.toml`
  - `Cargo.lock`
  - `rust/tests/checks/src/static_checks.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs`
  - `rust/dotfiles-cli/tests/secrets_internal_stub/piv_io_internal_stub.rs`
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
  - `rust/dotfiles-cli/src/secrets/support/aead.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/oaep.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/sealed_blob.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/secret_random.rs`
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
  - `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`
  - `rust/dotfiles-cli/Cargo.toml`

## 実行コマンド

- `direnv exec . cargo check -p dotfiles-cli`
- `direnv exec . cargo xtask check`
- `direnv exec . cargo clippy --workspace --all-targets`
- `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application`
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli`
- `direnv exec . cargo test -p dotfiles-cli secrets::application:: --lib`
- `direnv exec . cargo fmt --check`
- `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::support::`
- `direnv exec . cargo clippy -p dotfiles-cli --all-targets`
- `git diff --check`

## 結果要約

- `cargo check`: 実行済み
- `cargo test --no-run`: 実行済み
- `cargo xtask check`: 実行済み（`5217c7f`）
- `cargo clippy --workspace --all-targets`: 実行済み（`5217c7f`）
- `RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 実行済み（`5217c7f`）
- `cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application`: 成功（43 passed, 0 failed）
- `cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli`: 成功（22 passed, 0 failed）
- `cargo test -p dotfiles-cli secrets::application:: --lib`: 成功（0 passed, 40 filtered out）
- `cargo fmt --check`: 成功
- `cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::support::`: 成功（19 passed, 0 failed, 64 filtered out）
- `cargo check -p dotfiles-cli`: 成功
- `cargo clippy -p dotfiles-cli --all-targets`: 成功
- `git diff --check`: 成功
- 状態: `再レビュー待ち`

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
- 確認前提: この節は履歴サイクル（2026-05-26）専用記録であり、現行サイクル（2026-05-27）の reviewer 判定には使用しない。
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

## 履歴レビュー結果（現行サイクル判定対象外）

- `review-security-2026-05-25.md`、`review-operational-2026-05-25.md`、`review-spec-2026-05-25.md`、`review-test-2026-05-25.md`、`review-doc-2026-05-25.md`、`structural-review.md` は履歴サイクル用 artifact として保持する。
- 上記 artifact の判定結果は current-cycle（2026-05-27）の reviewer 状態を前進させる根拠として使用しない。

## 2026-05-27 e148c0d 検証追記

- 対象コミット: `e148c0d fix(secrets): YubiKey再レビュー指摘を修正`
- `direnv exec . cargo xtask check`: 成功
- `direnv exec . cargo clippy --workspace --all-targets`: 成功
- `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 成功
- 状態: `再レビュー待ち`

## 2026-05-27 41084ae 検証追記

- 対象コミット: `41084ae fix(secrets): adapter境界のclippy指摘を修正`
- `cargo check -p dotfiles-cli`: 成功
- `git diff --check`: 成功
- `direnv exec . cargo xtask check`: 成功
- `direnv exec . cargo clippy --workspace --all-targets`: 成功
- `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 成功
- 状態: `再レビュー待ち`

## 2026-05-27 78f10ac 検証追記

- 対象コミット: `78f10ac refactor(secrets): object逆引き規則をdomainへ移管`
- 修正内容: PIV object ID から `SecretName` への逆引き規則を adapter stub から `domain::piv::SecretName::from_object_id` へ移した。
- `cargo check -p dotfiles-cli`: 成功
- `git diff --check`: 成功
- 状態: `再レビュー待ち`

## 2026-05-27 9ff38d7 検証追記

- 対象コミット: `9ff38d7 refactor(secrets): 上書き可否規則をdomainへ移管`
- 修正内容: `put` の既存 secret 上書き可否判定を `domain::piv::SecretName::ensure_write_allowed` へ移した。
- `cargo check -p dotfiles-cli`: 成功
- `git diff --check`: 成功
- 状態: `再レビュー待ち`

## 2026-05-27 adapter 構造説明同期

- 対象コミット: `959269a docs(secrets): adapter構造修正の検証証跡を追記`
- 確認内容: current-cycle の structural 修正説明を実コードの現構成へ同期した。
- 現構成:
  - entrypoint は `SecretsAdapters::default()` を利用する。
  - `adapters::real_secrets_boundary()` は存在しない。
  - `JsonReportAdapter` と公開 constructor は存在しない。
  - report 翻訳は `ReportPort for RealSecretsBoundary` の trait 実装境界に閉じている。
- 状態: `再レビュー待ち`

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

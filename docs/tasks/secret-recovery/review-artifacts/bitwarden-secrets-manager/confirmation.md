# Bitwarden Secrets Manager 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Secrets Manager` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `Hypatia 後 fresh review 前確認済み`
- 判定位置づけ: `デザインPR段階 current-cycle 差分の fresh review 前確認（作業項目全体の完了判定ではない）`
- 対象差分識別子: `2026-05-29-hypatia-current-cycle-worktree@HEAD-dccada7`
- 対象ブランチ: `feat/bitwarden-secrets-manager`
- 確認開始時点参照: `../../work-items/bitwarden-secrets-manager.md` 記載の `現行サイクル差分識別子`
- 差分区分: `実装`
- 確認 scope: BSM 実装確認対象は本作業項目の対象コードパス、BSM へ直接関係する文書差分、必要な実検証に限定する。同一未コミット worktree に残るその他の文書差分は対象外差分であり、BSM current-cycle の確認結果、レビュー合格根拠、commit 着手 gate の根拠として扱わない。対象パス exact list、root/area 台帳、current-cycle 文言の完全同期は補助記録であり gate ではない。

## 確認手順と結果

- 手順:
  - `direnv exec . cargo fmt -p dotfiles-cli -- --check`
  - `direnv exec . cargo check -p dotfiles-cli`
  - `direnv exec . cargo test -p dotfiles-cli --lib`
  - `direnv exec . cargo test -p dotfiles-cli --lib bws`
  - `git diff --check`
  - app 層 `secrets-internal-test-stub` 残存検索
- 結果:
  - `cargo fmt -p dotfiles-cli -- --check` 成功
  - `cargo check -p dotfiles-cli` 成功
  - `cargo test -p dotfiles-cli --lib` 成功（112 passed）
  - `cargo test -p dotfiles-cli --lib bws` 成功（28 passed）
  - `git diff --check` 成功
  - app 層 `secrets-internal-test-stub` 残存検索は該当なし
- 未実施理由（未実施がある場合）: `なし`

## 実装進捗への影響

- 対象コードパス差分:
  - `rust/dotfiles-cli/src/secrets/domain/values.rs` — `BwsSecretName`、`RestoreGpgCommand`、`RestorePassCommand` 追加
  - `rust/dotfiles-cli/src/secrets/ports.rs` — `BwsClientPort` trait 追加
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs` — BWS check 実装（`BwsClientPort` 経由でトークン読み出し＋両 BWS secret fetch）
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_prompt.rs` — application 層の feature-gated inline test 除去
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_stdin_json.rs` — application 層の feature-gated inline test 除去
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_prompt.rs` — application 層の feature-gated inline test 除去
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_stdin_json.rs` — application 層の feature-gated inline test 除去
  - `rust/dotfiles-cli/src/secrets/application/run_get_with.rs` — application 層の feature-gated inline test 除去
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_prompt.rs` — application 層の feature-gated inline test 除去
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_stdin.rs` — application 層の feature-gated inline test 除去
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_prompt.rs` — BWS token rotate prompt 経路
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_stdin.rs` — BWS token rotate stdin 経路
  - `rust/dotfiles-cli/src/secrets/application/run_setup_with.rs` — application 層の feature-gated inline test 除去
  - `rust/dotfiles-cli/src/main.rs` — async CLI entrypoint
  - `rust/dotfiles-cli/src/lib.rs` — async library dispatch 境界
  - `rust/dotfiles-cli/src/cli.rs` — async secrets dispatch 呼び出し
  - `rust/dotfiles-cli/src/secrets/application.rs` — 新規 module 宣言追加
  - `rust/dotfiles-cli/src/secrets.rs` — BWS 関連 command ルーティングと `BwsClientPort` bound 追加
  - `rust/dotfiles-cli/src/secrets/entrypoint.rs` — composition root 境界で adapter 生成と port 束ねを保持
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
  - `rust/dotfiles-cli/tests/secrets_cli.rs` — BWS CLI 経路確認対象
  - `rust/dotfiles-cli/tests/secrets_internal_stub/piv_io_internal_stub.rs` — internal feature test support
  - `rust/dotfiles-cli/Cargo.toml` — Bitwarden SDK / tokio / internal feature dependency
  - `Cargo.toml` / `Cargo.lock` — workspace dependency resolution
- 補助記録メモ: `docs/tasks/secret-recovery/tasks.md` の固定実装単位トラッカーと本記録は参照補助に使えるが、状態文言の exact 同期そのものは gate ではない。
- 前進可否メモ: Hypatia 後の現行差分は fresh review 未実施。必須レビュー担当の再レビュー合格と集約判定が揃うまで commit 前進不可。

## セキュリティ確認結果

- 秘密値/認証情報の露出確認: `fresh review 前確認済み` — `BwsClientAdapter` は `support/protection/bws.rs` の BWS 専用操作へ SDK get と返却 value の `ProtectedSecret` 化を委譲し、adapter 側で `secret.value` の平文受け取りを行わない。固定 secret key/name、一意解決、0件/複数件 failure 化、外部確認 plan は support へ移していない。secret 値をログ/エラー本文へ出力しない方針は維持する。
- ログ/引数/一時ファイル/stdout/stderr 確認: `完了` — SDK 呼び出し失敗時の user-visible error は固定要約のみを返し、secret 値や raw API 応答本文を埋め込まない。
- 権限境界/永続化/失敗時挙動確認: `fresh review 前確認済み` — 通常ビルドの `BwsClientAdapter` は SDK 経路のみを持つ。token は `ProtectedSecret` 借用境界内で処理し、SDK が所有 plaintext buffer の move を要求する login request と secret value 取得後の保護値化は `support/protection` 内の BWS 専用操作で完了する。永続化なし。

## 実装継続確認（2026-05-29）

- 対象差分識別子: `2026-05-29-uncommitted-current-worktree`
- 実装補正:
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/report_adapter.rs` — JSON report の pretty 出力を `ReportPort` adapter 内の private helper に閉じ、`support` へ report 語彙を移さない構造に補正した。
  - `rust/dotfiles-cli/src/secrets/support.rs` / `rust/dotfiles-cli/src/secrets/support/json_report.rs` — `support::json_report` module を除去し、support 層を process-generic helper / protection backend 境界に限定した。
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs` — BWS project/secret lookup failure が fetch 続行せず failed report へ収束する application tests を追加した。
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs` — BWS lookup failure の application test 経路を追加した。
  - `rust/dotfiles-cli/src/secrets/adapters/bws_client.rs` — `protected_access_token` / `parse_uuid` helper の責務境界 doc comment を補足した。
- 確認手順と結果:
  - `direnv exec . cargo fmt -p dotfiles-cli -- --check` 失敗（整形差分検出）
  - `direnv exec . cargo fmt -p dotfiles-cli` 成功
  - `direnv exec . cargo fmt -p dotfiles-cli -- --check` 成功
  - `direnv exec . cargo check -p dotfiles-cli` 成功
  - `direnv exec . cargo test -p dotfiles-cli --lib secrets::adapters::bws_client::tests` 成功（3 passed）
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application::run_verify_yubikey_with::tests` 成功（7 passed）
  - `direnv exec . cargo test -p dotfiles-cli --lib secrets::application::run_verify_yubikey_with::tests::verify_executes_bws_external_check_when_requested` 成功（0 tests。対象 test は `secrets-internal-test-stub` feature gate 下のため feature なしでは実行対象なし）
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application::run_verify_yubikey_with::tests::verify_executes_bws_external_check_when_requested` 成功（1 passed）
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application` 成功（63 passed）
  - `git diff --check` 成功
- 未実施理由（未実施がある場合）: `なし`

## James 是正後 fresh review 前確認（2026-05-29）

- 対象差分識別子: `2026-05-29-james-current-cycle-worktree@HEAD-dccada7`
- 比較範囲: `HEAD` = `dccada7` を基点にした未コミット worktree 差分（未コミット tracked diff と未追跡 `rust/dotfiles-cli/src/secrets/runtime.rs` を含む）。保存済み commit 終端そのものを review 対象終端とは扱わない。
- scope 補足: BSM current-cycle の確認/review scope は本記録冒頭の `確認 scope` に限定する。同一 worktree 上の対象外文書差分は、BSM の確認結果・レビュー合格根拠・commit 着手 gate の根拠に含めない。
- 実装補正:
  - `rust/dotfiles-cli/src/secrets/adapters/bws_client.rs` — `fetch_bws_secret_by_id` から `secrets().get()` と `secret.value` の受け取りを除去し、adapter 側は BWS secret ID を protection 境界へ渡す構造へ変更した。
  - `rust/dotfiles-cli/src/secrets/support/protection/bws.rs` — `get_protected_secret_value(id)` を追加し、BWS SDK get と返却 value の `ProtectedSecret` 化を protection 境界内で完了する構造へ変更した。
  - 固定 secret key/name、一意解決、0件/複数件 failure 化、`verify-yubikey --check bws` 相当の外部確認 plan は support へ移していない。
- 確認手順と結果:
  - `direnv exec . cargo fmt -p dotfiles-cli -- --check` 成功
  - `direnv exec . cargo check -p dotfiles-cli` 成功
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application::run_verify_yubikey_with` 成功（7 passed）
  - `direnv exec . cargo test -p dotfiles-cli --lib secrets::adapters::bws_client` 成功（3 passed）
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::application` 成功（66 passed）
  - `git diff --check` 成功
- レビュー状態: `未実施` — この James 是正後差分は fresh review 開始前であり、集約判定は未確定。
- 未実施理由（未実施がある場合）: `fresh review は次工程で必須。確認コマンドの未実施はなし。`

## Hegel/Linnaeus 後 fresh review 前確認（2026-05-29）

- 対象差分識別子: `2026-05-29-linnaeus-current-cycle-worktree@HEAD-dccada7`
- 比較範囲: `HEAD` = `dccada7` を基点にした未コミット worktree 差分（未コミット tracked diff と未追跡 `rust/dotfiles-cli/src/secrets/entrypoint.rs` を含む）。保存済み commit 終端そのものを review 対象終端とは扱わない。
- scope 補足: BSM current-cycle の確認/review scope は本記録冒頭の `確認 scope` に限定する。同一 worktree 上の対象外文書差分は、BSM の確認結果・レビュー合格根拠・commit 着手 gate の根拠に含めない。
- Hegel 文書補正:
  - storage backend が暗号化・復号・sealed blob を内包する場合、port は sealed blob 形式や暗号方式ではなく datastore capability を公開することを正本へ明記した。
  - `support/protection` に置けるものを backend 内部の暗号化・復号・sealed blob・protection・zeroize・core dump 保護などの技術境界に限定した。
  - setup 判定、必須 secret 判定、固定 key/name/role の意味づけ、一意解決、0件/複数件 failure、取得対象の過不足、外部確認 plan は support に逃がせない基準として明記した。
- Linnaeus 実装補正:
  - application 層から `secrets-internal-test-stub` bridge と feature-gated inline tests を除去した。
  - `runtime` module 参照は残っておらず、既存の `entrypoint` composition root 境界に収めた。
  - `piv::decrypt_data(...)` の戻り値を `support/protection/sealed_blob.rs` 側で `Zeroizing<Vec<u8>>` のまま保持して unwrap する境界へ移動した。
  - `adapters.rs` の module comment を adapter 型公開境界の説明へ整合した。
  - `report_adapter.rs` は JSON 変換だけではなく、`println!` による外部出力 emit 境界として残す。
  - `StorageAdapter::inspect_secret_storage_setup` は raw datastore/device 観測値の取得に寄せ、setup 判定は domain intent 側に置いた。
- 確認手順と結果:
  - `direnv exec . cargo fmt -p dotfiles-cli -- --check` 成功
  - `direnv exec . cargo check -p dotfiles-cli` 成功
  - `direnv exec . cargo test -p dotfiles-cli --lib` 成功（49 passed）
  - `direnv exec . cargo test -p dotfiles-cli --lib bws` 成功（9 passed）
  - `git diff --check` 成功
  - app 層 `secrets-internal-test-stub` 残存検索は該当なし
- レビュー状態: `未実施` — この Linnaeus 後差分は fresh review 開始前であり、集約判定は未確定。
- 未実施理由（未実施がある場合）: `fresh review は次工程で必須。確認コマンドの未実施はなし。`

## Aristotle 後 fresh review 前確認（2026-05-29）

- 対象差分識別子: `2026-05-29-aristotle-current-cycle-worktree@HEAD-dccada7`
- 比較範囲: `HEAD` = `dccada7` を基点にした未コミット worktree 差分（未コミット tracked diff と未追跡 `rust/dotfiles-cli/src/secrets/entrypoint.rs` を含む）。
- scope 補足: BSM current-cycle の確認/review scope は本記録冒頭の `確認 scope` に限定する。同一 worktree 上の対象外文書差分は、BSM の確認結果・レビュー合格根拠・commit 着手 gate の根拠に含めない。
- 実装補正:
  - `rust/dotfiles-cli/src/secrets/application.rs` — 実装本体を持たない module root から usecase 単位テスト集約を除去した。
  - 当時の app 層共有 test helper 方針は後続差分で撤回済み。現行実装参照として扱わない。
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_prompt.rs` — primary enroll prompt の store / PIN / setup failure / store failure / verify failure / empty secret 停止条件テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_primary_with_stdin_json.rs` — stdin JSON primary enroll の PIN / verify failure / stdin-json error / PIN verification failure テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_prompt.rs` — spare enroll prompt の serial conflict / primary→spare 解決順序 / primary read before spare setup / PIN / verify failure / empty secret 停止条件テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_enroll_spare_with_stdin_json.rs` — spare stdin JSON の serial conflict / PIN / verify failure / stdin-json error / PIN verification failure テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_get_with.rs` — get の load→output と PIN 要否テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_prompt.rs` — prompt put の store / secret read failure / storage preflight before read / noninteractive prompt rejection / repeated write テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_put_with_stdin.rs` — stdin put の store / serial required / storage preflight before read / stdin error テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_prompt.rs` — rotate prompt の serial resolution / storage preflight before token read / verify report / invalid existing storage stop / PIN / continuation / already updated rejection / preservation テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_stdin.rs` — rotate stdin の serial required / storage preflight before token read / verify report / invalid existing storage stop / PIN テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_setup_with.rs` — setup の serial resolution 後 initialize / inspection failure / serial required / management auth precondition テストを復元した。
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs` — verify の PIN / BWS external check / conflicting option / local storage failure / BWS project lookup failure / required secret missing / fetch failure テストを復元した。
  - `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs` — app 層からの依存をなくすため削除した。現行方針では app 層共有 test support file も作らず、port trait 由来の `mockall` mock を各 `run_*.rs` の test 内で直接使う。
- 確認手順と結果:
  - `direnv exec . cargo fmt -p dotfiles-cli -- --check` 成功
  - `direnv exec . cargo check -p dotfiles-cli` 成功
  - `direnv exec . cargo test -p dotfiles-cli --lib` 成功（112 passed）
  - `direnv exec . cargo test -p dotfiles-cli --lib bws` 成功（28 passed）
  - `git diff --check` 成功
  - `rg -n "secrets-internal-test-stub" rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application` は該当なし
  - `rg -n "tests/secrets_application|/tests/|#\\[path\\s*=|include!\\(" rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application -S` は該当なし
- レビュー状態: `未実施` — この Aristotle 後差分は fresh review 開始前であり、集約判定は未確定。文書差分を含むため参照整合レビューを必須レビュー集合に含める。
- 未実施理由（未実施がある場合）: `fresh review は次工程で必須。確認コマンドの未実施はなし。`

## Hypatia 後 fresh review 前確認（2026-05-29）

- 対象差分識別子: `2026-05-29-hypatia-current-cycle-worktree@HEAD-dccada7`
- 比較範囲: `HEAD` = `dccada7` を基点にした未コミット worktree 差分（未コミット tracked diff と未追跡 `rust/dotfiles-cli/src/secrets/entrypoint.rs` を含む）。
- scope 補足: BSM current-cycle の確認/review scope は本記録冒頭の `確認 scope` に限定する。同一 worktree 上の対象外文書差分は、BSM の確認結果・レビュー合格根拠・commit 着手 gate の根拠に含めない。
- 実装補正:
  - `rust/dotfiles-cli/src/secrets/application.rs` — `tests/` 配下の app test support を `include!` する bridge を削除し、app 層内 test-only module 宣言へ変更した。
  - `rust/dotfiles-cli/src/secrets/application/app_test_support.rs` — 現行方針では禁止対象であり、現存実装として扱わない。app usecase test は各 `run_*.rs` の `#[cfg(test)] mod tests` 内で port trait 由来の `mockall` mock を直接組む。
  - `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs` — app 層から `tests/` 配下へ依存しないため削除した。
  - `docs/architecture/hexagonal-implementation-rules.md` / `docs/architecture/review-checklist.md` / `docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md` — app inline/unit test が `tests/` 配下へ依存しないこと、app 層共有 test support file を作らないこと、port trait 由来の `mockall` mock を各 test で直接使うこと、`ProtectedSecret` の test-only 最小アクセス許可を production API へ拡大しないことを整合させた。
- 確認手順と結果:
  - `rg -n 'tests/secrets_application|secrets_application/app_test_support|/tests/|#\\[path\\s*=|include!\\(' rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application -S` は該当なし。
  - `rg -n 'secrets-internal-test-stub' rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application -S` は該当なし。
  - `rg -n 'app_test_support|MockAppEventExpectation|expect_hit_event|expect_event_times|mockall::mock!' rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application -S` は現行実装では該当なしとする必要がある。
  - `direnv exec . cargo fmt -p dotfiles-cli -- --check` 成功。
  - `direnv exec . cargo check -p dotfiles-cli` 成功。
  - `direnv exec . cargo test -p dotfiles-cli --lib` 成功（112 passed）。
  - `direnv exec . cargo test -p dotfiles-cli --lib bws` 成功（28 passed）。
  - `git diff --check` 成功。
- レビュー状態: `未実施` — この Hypatia 後差分は fresh review 開始前であり、集約判定は未確定。文書差分を含むため参照整合レビューを必須レビュー集合に含める。
- 未実施理由（未実施がある場合）: `fresh review は次工程で必須。確認コマンドの未実施はなし。`

## BWS port mock / app usecase test 差し戻し確認（2026-05-29）

- 実装補正:
  - `BwsClientPort` を port trait 由来の test-only `mockall::automock` 対象に変更し、BWS app test から手書き `UnusedBwsClient` を削除した。
  - `run_verify_yubikey_with.rs` に BWS 成功、project lookup 失敗、secret lookup 失敗、fetch 失敗、report 反映、BWS port 呼び出し順序の tests を復元した。
  - `run_rotate_bws_token_with_prompt.rs` / `run_rotate_bws_token_with_stdin.rs` に token 更新前検証、token 読み取り前停止、store、更新後 verify、report、継続/停止条件の tests を復元した。
  - `run_get_with.rs` / `run_put_with_prompt.rs` に app usecase の port 呼び出し順序と停止条件の tests を復元した。
  - `ProtectedSecret` の平文取り出しは crate-wide API としては公開せず、BWS login、PIV PIN、sealed blob、stdout 出力ごとの `support/protection` 専用操作へ寄せた。
- 確認手順と結果:
  - `test ! -e rust/dotfiles-cli/src/secrets/application/app_test_support.rs` 成功。
  - `rg -n 'app_test_support|tests/secrets_application|secrets_application/app_test_support|/tests/|#\\[path\\s*=|include!\\(|secrets-internal-test-stub' rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application -S` は該当なし。
  - `rg -n 'UnusedBwsClient|MockApp|MockAppEventExpectation|expect_hit_event|expect_event_times|mockall::mock!|PortFuture' rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application rust/dotfiles-cli/src/secrets/ports.rs -S` は該当なし。
  - `direnv exec . cargo fmt -p dotfiles-cli -- --check` 成功。
  - `direnv exec . cargo check -p dotfiles-cli` 成功。
  - `direnv exec . cargo test -p dotfiles-cli --lib` 成功（65 passed）。
  - `direnv exec . cargo test -p dotfiles-cli --lib bws` 成功（17 passed）。
  - `git diff --check` 成功。
- レビュー状態: `未実施` — この差し戻し後差分は fresh review 開始前であり、集約判定は未確定。

## PR #33 / Issue #30 task-list-outside 確認（2026-05-30）

- 対象位置づけ: `PR #33 / Issue #30 の branch 作り直しおよび構造レビュー・ドキュメントレビュー・運用整合レビュー差戻し補正の確認。Bitwarden Secrets Manager 作業項目の Hypatia 以前の current-cycle 確認とは別の task-list-outside 記録として扱う。`
- 固定対象差分: `base 5ff5e54..head 2ececf1`（PR #33 作り直し時点の保存済み commit 差分）
- 補正対象差分: `2ececf1..この補正 HEAD`（adapter root 再公開除去、adapter-local stub doc comment 補正、test-review skill 正本参照化、PR #33 task-list-outside 証跡追加）
- 対象ブランチ: `refactor/secrets-structure-issue-30-main`
- 確認した commit linkage:
  - `git rev-parse --short HEAD` は着手時 `2ececf1`。
  - `git branch --show-current` は `refactor/secrets-structure-issue-30-main`。
  - `git diff --name-only 5ff5e54..2ececf1` により、PR #33 の保存済み対象差分を再特定した。
- 確認手順と結果:
  - `cargo fmt --all` 成功。
  - `rg -n "pub\\(crate\\) use|pub\\(super\\) use|adapters::(DeviceSelectionAdapter|StorageAdapter|ProcessIoAdapter|JsonReportAdapter|BwsClientAdapter)|tests 配下の double|include する" rust/dotfiles-cli/src/secrets .agents/skills/test-review -S` 実行。adapter root の再公開、旧 `tests 配下の double を include` 文言、呼び出し側の `adapters::Type` 依存は残存なし。`ports.rs` の port 契約再公開だけが別層の既存一致として残る。
  - `rg -n "secrets-internal-test-stub|#\\[path\\s*=|include!\\(|tests/secrets_application|app_test_support|mockall::mock!|UnusedBwsClient|PortFuture" rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application rust/dotfiles-cli/src/secrets/ports.rs -S` 実行。application 層の internal stub bridge / tests 配下依存 / 手書き mock 残存なし。
  - `rg -n "canonical internal backend stub|production build exclusion|production build 非混入|fixture/state helper|runtime 分岐なし|unchanged production command path" .agents/skills/test-review/SKILL.md .agents/skills/test-review/SKILL_ja.md -S` 実行。skill 側は正本参照文言のみで、詳細条件列挙の残存なし。
  - `direnv exec . cargo check -p dotfiles-cli` 成功。
  - `direnv exec . cargo check -p dotfiles-cli --features secrets-internal-test-stub` 成功。
  - `direnv exec . cargo test -p dotfiles-cli --lib secrets::adapters` 成功（2 passed）。
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --lib secrets::adapters` 成功（2 passed）。
  - `git diff --check` 成功。
- レビュー状態: `差戻し補正後 fresh review 未実施`。この確認記録は対象差分と実行確認を追跡可能にする補助記録であり、必須レビュー担当の合格や集約判定を代替しない。
- 未実施理由（未実施がある場合）: `なし`

# BSM テストレビュー記録（2026-05-30）

- レビュー担当: テストレビュー担当
- 対象: `refactor/secrets-structure-issue-30-main` / HEAD `a1a36cc`（作業ツリー clean）
- 差分: `git diff 5ff5e54..a1a36cc`（PR #33 / Issue #30）
- 主対象: `rust/dotfiles-cli/tests/secrets_cli.rs`、`rust/dotfiles-cli/src/secrets_internal_test_stub_contract.rs`、`rust/dotfiles-cli/src/secrets/adapters/bw/internal_stub.rs`、`rust/dotfiles-cli/src/secrets/adapters/yubikey/selected_device.rs`、`src/secrets/**` の `#[cfg(test)] mod tests`

判定: 合格
判定要約: 所見なし

## 完了条件の分類とテスト網羅

### テストで検証すべき項目（網羅確認済み）

- BWS 外部確認経路（`verify-yubikey --check bws`）: `verify_yubikey_runs_bws_external_check`（secrets_cli.rs:422）で最終 BWS datastore 観測の `resolved_secrets` を検証。`verify_yubikey_runs_with_yubikey_path`（:405）は `--check` 無しで BWS 観測が出ないこと（`has_bws_observation()==false`）を確認。
- 固定 secret name の一意解決 / 0件 / 複数件 failure（domain rule）: `domain/bws.rs` の inline unit test（:177 unique、:187 not-found、:202 multiple、:219 secret unique、:233 not-found、:248 multiple）で網羅。default build で 84 unit test passed。
- secret 入出力 modality（prompt / stdin / stdin-json）と YubiKey 保存後の最終 datastore: enroll-primary/spare、put、get、rotate-bws-token の pipe/PTY 双方を網羅し `assert_stored_secret` で最終観測を検証（secrets_cli.rs:114-402, 557-633）。
- 内部 stub route が production env 選択を持たないことの監査: `verify_yubikey_audits_stub_route_in_internal_stub_build`（:528）が spec env 未設定時に失敗することを確認。
- 統合テスト実行結果: `--features secrets-internal-test-stub` で 25 test passed、default lib で 84 test passed（実行確認済み）。

### 構造確認・文書確認で満たす項目（テスト網羅を要求しない）

- SDK 呼び出しの adapter/port 隔離、`application` は順序制御のみ、`domain` は SDK/I-O 型非依存: 層配置の構造条件であり、テスト網羅対象ではない（構造/アーキ整合レビュー職責）。
- `ProtectedSecret` 生値アクセスを公開 API にしない: `to_test_bytes`/`from_test_bytes` は `#[cfg(test)]` または `secrets-internal-test-stub` gate に閉じ、production build へ公開されない（protection.rs:134-150）。test-only 観測口として規約許容範囲。

## test double / internal backend stub の責務判定

- BWS port stub（`adapters/bw/internal_stub.rs`）と YubiKey port stub（`adapters/yubikey/selected_device.rs`）は別 datastore（`BWS_DATASTORE` / `YUBIKEY_DATASTORE`）・別 spec env・別 observation frame を持ち独立。共有 StubState / 共有 state file での結合なし。
- 両 stub は `#[cfg(feature = "secrets-internal-test-stub")]` で compile-time selection。`#[cfg(not(feature))]` 側に real backend を置き runtime real/stub 分岐を作らない（bw.rs:6-32, yubikey.rs:14-15,121-）。
- 責務は port 契約（`BwsClientPort` / `SelectedDeviceDiscoveryIo` / `SecretDeviceIo`）の datastore 境界翻訳に一致し、adapter 層責務を超えていない。production command path を変形していない（同一 `dotfiles` binary を実行）。
- 最終 datastore 観測は `STUB_OBSERVATION_PREFIX` の stdout sentinel line（bw/internal_stub.rs:184-195, selected_device.rs:266-277）。test-only 観測面でダミー secret 値を含むが production build には compile されず本物 secret 出力経路ではない。hidden temp / output path / shared state file への secret 残留なし。
- 共有 contract（`secrets_internal_test_stub_contract.rs`）は env 名と sentinel prefix の単一定義のみで、backend datastore schema / fixture 展開 / state helper を公開しない。
- 統合テスト側（`secrets_cli.rs`）は port ごとの初期条件 spec（fixture 名）の投入と、stdout 最終観測 JSON の parse/assert のみを保持。backend state schema・状態遷移 helper・write event helper・bincode schema・backend 内部保存形式を test 側に保持していない。stub 内部遷移の観測 assertion なし。
- 旧 test-side state 保持（`tests/secrets_internal_stub/cli_stub_state.rs`、`bws_client_internal_stub.rs`、`piv_io_internal_stub.rs`）は削除済みで、現行 tree に test 側 backend state/schema/helper は残存しない。
- app 層 use case orchestration test は internal stub feature と切り離されている。`application/` 配下に `secrets-internal-test-stub` gate / bridge / 共有 test support file（`app_test_support.rs`）は存在しない。

## Issue #30 構造変更スコープの確認

- 構造変更（port/domain/adapter 再分割、stub の port 別分離、stdout 観測化）に伴う import/path/fixture 配置の変更が中心であり、テストケース本体の不当な追加/削除/期待値変更は確認されない。`secrets_cli.rs` の差分は旧 helper 参照から contract 経由 spec/observation への移行と、PTY 待機の構造是正に対応する。

## 根拠（チェック項目と結果）

- 完了条件をテスト項目/構造・文書項目へ分類し、前者のみテスト網羅を要求した: 上記分類のとおり全項目に対応テスト存在。
- internal backend stub を形式でなく責務で判定: adapter 層 datastore 翻訳責務に一致、production path 非変形、compile-time selection、test 側責務逸脱なし → 不合格根拠なし。
- BWS/YubiKey port stub 独立: 確認済み。
- test 側 backend state/schema/helper 保持: なし。
- production 層 inline unit test の存在のみを理由に不合格にしない原則を適用: domain/bws.rs ほかの inline test は self-module 検証であり許可。
- 実検証: stub feature 25 test passed、default lib 84 test passed。

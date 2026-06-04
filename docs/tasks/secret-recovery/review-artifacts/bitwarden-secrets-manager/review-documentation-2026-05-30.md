# ドキュメントレビュー判定 — Bitwarden Secrets Manager（2026-05-30）

- レビュー担当: ドキュメントレビュー担当
- 対象リポジトリ/ブランチ: `/Users/ya/works/dotfiles` / `refactor/secrets-structure-issue-30-main`
- 対象 HEAD: `a1a36cc`（作業ツリー clean）
- 現行サイクル差分: `git diff 5ff5e54..a1a36cc`（base `5ff5e54` / 終端 `a1a36cc`）。PR #33 / Issue #30
- 判定対象: `rust/dotfiles-cli/src/secrets/**` のコード内ドキュメントコメント（`///`・`//!`・`/** */`）の実装整合・why/責務境界説明
- 参照した正本: `docs/architecture/hexagonal-implementation-rules.md`「ドキュメントコメント規則」、`docs/docs-governance.md`、`docs/task-governance/implementation-review-judgement.md`（ドキュメントレビュー担当 職責）

判定: 合格
判定要約: 所見なし
根拠:

- **`secrets-internal-test-stub` stub backend 接続箇所のコメント**: `rust/dotfiles-cli/src/secrets/adapters/bw.rs:6-12` と `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs:6-15` の双方で、`#[cfg(feature = "secrets-internal-test-stub")] mod ...` 宣言直上に「production build 非混入」「runtime real/stub 分岐を作らない」「compile-time feature selection」「integration test は adapter stub module を import せず同じ `dotfiles` binary を実行」「初期条件 spec JSON と最終状態観測 JSON だけを外部観測面とする」を明記。Issue #30 が要求する stub 接続箇所の必須 code comment 内容を満たす。
- **adapter 配下 `stub` module 本体のヘッダ**: `adapters/bw/internal_stub.rs:1-11` と `adapters/yubikey/selected_device.rs:1-11` の `//!` で、feature 専用・production 非 compile・compile-time selection・stdout sentinel observation・YubiKey/BWS port stub 間で state/schema/file 非共有を明記。port-stub 独立境界が doc comment 上も説明されている。
- **stub backend が state を読む境界（datastore/observation）**: `adapters/bw/internal_stub.rs:8-9`（spec env から private datastore 展開、最終観測 JSON を stdout sentinel に出力）と `secrets_internal_test_stub_contract.rs:1-7`（feature 限定の env 名・stdout observation framing 共有、backend datastore schema / fixture expansion / state helper は非公開）が、観測境界の責務を説明している。stdout 観測が test-only 明示観測面である旨は `tests/secrets_cli.rs:1-6` の file-level header（production command path は runtime env による real/stub 選択を持たない）と整合する。
- **integration test 側 fixture/state helper**: `tests/secrets_cli.rs:1-6` が feature-gated internal stub の検証目的、production path に runtime real/stub 選択がないこと、port ごとの初期条件 spec JSON 投入と最終状態観測 JSON のみ検証することを file-level doc comment で説明。テストケース個別ヘッダの機械的必須化は行わず、ファイルヘッダ要件として充足。
- **`application/` 直下 `run_*` entrypoint**: `run_get_with.rs`、`run_verify_yubikey_with.rs`、`run_enroll_primary_with_prompt.rs`、`run_enroll_primary_with_stdin_json.rs`、`run_enroll_spare_with_prompt.rs`、`run_enroll_spare_with_stdin_json.rs` ほか各 `run_*.rs` の主要 entrypoint 関数に doc comment があり、先頭文の主契約に続けて「なぜその順序制御が必要か／どの責務境界を保護するためか」（例: 入力 I/O 変更を storage 手順から分離、復号・出力方針を adapter 側へ固定、外部確認の責任範囲を曖昧にしない）を明記。what の言い換えにとどまらない。
- **core workflow の非自明 internal type/function**: `domain/bws.rs` の `resolve_single_bws_lookup`、`BwsSecretName::resolve_id`、`BwsProjectName::resolve_id` 等が、固定 key/name の対象同一性が業務規則であること、0 件/複数件がともに domain failure であること、adapter に候補数判定を再実装させない理由を doc comment で説明。`support/protection/bws.rs` の `ZeroizingAccessTokenLoginRequest`・`BwsClientSession`・`get_protected_secret_value` が zeroize lifecycle、core dump 抑止境界、所有権規則、lookup rule を扱わない責務分界を具体名で記述。
- **port/adapter/support の層責務境界説明**: `ports/bw.rs:1-16`（BWS capability のみ宣言、SDK 認証・UUID 変換を adapter へ閉じる、caller/implementor 責任分界）、`ports/io.rs`・`ports/yubikey.rs`（capability と caller/implementor 責務分界）、`adapters/bw.rs:1-4`・`adapters/io.rs:1-4`・`adapters/yubikey.rs:1-4`（どの port をどの外部 API へ翻訳し、何を持たないか）、`domain/{bws,commands,enrollment,verification}.rs` の module header（業務意味のみ保持、report/CLI 表現を持たない）が、いずれも翻訳境界・契約境界・保護境界を説明している。
- 実装と矛盾するコメント・古いコメント・誤解を招くコメントは確認されず。必須対象（`run_*` entrypoint、非自明 internal type/function、port/adapter/support 層責務境界）で doc comment 欠落または what 言い換えのみの blocker は検出されなかった。テストケースへのヘッダコメント機械的必須化は適用していない。

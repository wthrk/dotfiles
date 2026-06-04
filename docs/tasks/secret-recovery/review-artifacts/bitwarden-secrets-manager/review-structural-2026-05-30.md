# 構造レビュー（Bitwarden Secrets Manager / Issue #30・PR #33）2026-05-30

- レビュー担当: 構造レビュー担当
- 対象: `/Users/ya/works/dotfiles`、branch `refactor/secrets-structure-issue-30-main`、HEAD `a1a36cc`（作業ツリー clean）
- 対象差分: `git diff 5ff5e54..a1a36cc`（作業定義の終端 `77dc03c` 以降に port別 internal stub 分離・stdout観測移行の構造補正 commit を含む）
- 対象モジュール: `rust/dotfiles-cli/src/secrets/` 全体、`tests/secrets_cli.rs`、`src/secrets_internal_test_stub_contract.rs`
- 参照: `docs/architecture/hexagonal-implementation-rules.md`、`docs/architecture/review-checklist.md`、`docs/task-governance/implementation-review-judgement.md`

判定: 合格
判定要約: 所見なし

## Step 1 — 哲学的検証（層別「レビュー時の問い」への回答）

### ports（`ports/bw.rs`, `ports/io.rs`, `ports/yubikey.rs`, `ports.rs`）
- このコードは「ドメインが何を必要とするかの宣言」だけをしているか → **Yes**。`BwsClientPort`（`ports/bw.rs:18`）は `list_bws_projects`/`list_bws_secrets`/`fetch_bws_secret_by_id` の capability 契約のみで、SDK 認証・UUID 変換・lookup rule を含まない（doc `ports/bw.rs:12-16` が adapter 責務へ明示委譲）。`SecretInputPort`（`ports/io.rs:28`）は modality enum ではなく `read_bws_access_token_secret` 等の capability 名で表現。`ProtectedSecret` を carrier として受け渡すのは secret-recovery で許可された port 境界用途で、平文取り出し API は持たない。
- fat port になっていないか → **No**。trait は backend 機能単位（bw/io/yubikey）に分割され、`ports.rs:6-8` の module 分割と対応。`ports.rs` の `pub(crate) use` 再公開は domain value / port contract 境界明確化のための再公開で許可範囲（hexagonal-implementation-rules `公開範囲と再公開規則`）。
- 結論: 哲学違反なし。

### domain（`domain/bws.rs`）
- ビジネスルールを domain の第一責務として表現しているか → **Yes**。固定 secret key（`domain/bws.rs:20-25`）、固定 project name（`:108-110`）、secret ID の一意解決と 0件/複数件 failure 化（`resolve_id` `:30-51`/`:115-125`、`resolve_single_bws_lookup` `:142-158`）が domain に閉じている。SDK 型・I/O 型へ非依存（opaque `BwsProjectId`/`BwsSecretId` は `String` のみ保持、`:58`/`:82`）。
- 技術語彙の混入はないか → **No**。`as_str`/`new` は opaque ID 境界変換のみ。
- 結論: 哲学違反なし。BWS の業務規則が support に逃げていない（最重要観点）ことをコードで確認した。

### application（`run_verify_yubikey_with.rs`, `run_get_with.rs` ほか）
- 順序と分岐だけを知っているか → **Yes**。`run_verify_yubikey_with`（`:19-168`）は `verify-yubikey --check bws` の外部検証 plan を application orchestration として保持し、一意解決は domain（`BwsProjectName::resolve_id` `:88-89`、`secret_name.resolve_id` `:119`）、過不足判定は domain（`check.required_bws_secrets()` `:115`）へ委譲する。port 呼び出し順序・停止条件・report 反映だけを持つ。
- concrete I/O / adapter 具体型 / 外部 SDK 型を持っていないか → **No**。port trait 経由のみ。`run_get_with`（`:12-42`）も同様で `println!`・stdin 読み取りなし。
- use case 独自型を定義していないか → **No**。扱う型は domain 型（`GetCommand`、`VerifyYubikeyCommand`、`VerifySummary` 等）のみ。
- 結論: 哲学違反なし。`verify-yubikey --check bws` plan は support ではなく application にある。

### adapters（`adapters/bw.rs`, `adapters/io.rs`, `adapters/yubikey.rs` ほか）
- 翻訳のみか → **Yes**。`adapters/bw.rs` は SDK project/secret list を `BwsLookupCandidate` へ変換し、`parse_uuid`（`:101`）は SDK 型変換失敗のみ扱い「ID 一意性・対象同一性は domain で判定済み」と明記（`:97-99`）。fetch は `support::protection::bws` の SDK get + 保護境界へ委譲（`:93`）。
- domain object のビジネス判断を adapter 内で実行していないか → **No**。`io/report.rs` は presentation（JSON key・status 文字列 `:47-74`）のみで summary semantics は domain。
- 結論: 哲学違反なし。

### support（`support.rs`, `support/protection/bws.rs`, `support/process_io.rs`）
- 共通技術 primitive か secret 保護境界 backend 操作か → **Yes**。`support/protection/bws.rs` は SDK login の所有 token buffer 作成・zeroize（`ZeroizingAccessTokenLoginRequest` `:17-49`）と SDK get + 保護所有値化（`get_protected_secret_value` `:61-69`）に限定され、doc が「lookup rule や 0件/複数件の failure 化を扱わない」「project/secret lookup rule や外部確認 plan は扱わない」と明記（`:58-60`、`:75`）。`support/process_io.rs` は process-generic な terminal/stdin/stdout 補助で YubiKey/use case 語彙を持たない（`:1-4`）。
- 固定 secret key/name の意味づけ・一意解決・0件/複数件 failure・過不足判定・`verify-yubikey --check bws` 外部検証 plan が support に置かれていないか → **置かれていない**。いずれも domain（`domain/bws.rs`）または application（`run_verify_yubikey_with.rs`）にある。
- 結論: 哲学違反なし。support は逃げ場として使われていない。

## Step 2 — チェックリスト適合

- 公開面（絶対規則）: `adapters/` 配下の `pub(in crate::secrets)`/`pub(super)` シンボルは port trait を実装する struct のみ（`io.rs:23`/`:71`、`yubikey.rs:37`/`:59`、`bw.rs:30`、`io/report.rs:20`、`yubikey/device_serial_adapter.rs:25`、`yubikey/storage_adapter.rs:30`、`io/process.rs:94`）。`parse_uuid`・datastore helper・JSON helper は private `fn`。非 port helper の `pub(crate)`/`pub(super)` 公開なし。
- application 依存方向・構成: 各 `run_*.rs` は `domain`/`ports` のみ依存、1 file = 1 `run_*`、`mod.rs`/`use_case.rs`/`#[path]` なし。`zeroize` は `support/protection` 配下のみ。
- internal backend stub（`adapters/bw/internal_stub.rs`, `adapters/yubikey/selected_device.rs`）: hexagonal-implementation-rules `internal backend stub の配置` の全条件をコード根拠で確認 — (a) `#[cfg(feature = "secrets-internal-test-stub")]` 限定で production build 非混入（`bw.rs:6-7`、`yubikey.rs:14-15`）、(b) runtime real/stub 分岐なし、production command path 単一（`secrets.rs:147` `RuntimePorts::production()`、`secrets.rs:164-175` に compile-time gate のみ）、(c) port trait 契約で usecase 駆動（`internal_stub.rs:57-80`）、(d) BWS stub（`BWS_DATASTORE` `internal_stub.rs:55`、env `BWS_STUB_SPEC_ENV`、frame `port:"bws"`）と YubiKey stub（別 datastore・`YUBIKEY_STUB_SPEC_ENV`・`port:"yubikey"`）が state/schema/file 非共有（`internal_stub.rs:11`/`selected_device.rs:11` に明記）、(e) 最終観測は stdout sentinel（`internal_stub.rs:184-195`、`STUB_OBSERVATION_PREFIX`）で hidden temp/output path/shared state file へ secret を残さない、(f) integration test は stub module を import せず contract module（env 名+prefix）のみ参照し feature 有効の `dotfiles` binary を実行（`tests/secrets_cli.rs:16-19`、`:76-99`）、(g) test 側は初期 spec JSON 投入（`tests/secrets_cli.rs:775-836` の fixture 名 spec）と最終 observation JSON 観測（`:838-875`）のみで backend state schema/状態遷移 helper/write event helper/bincode schema を保持しない。fixture 展開は adapter stub 側（`internal_stub.rs:197-236`、`datastore_from_spec`）に閉じる。`to_test_bytes`（`protection.rs:145-150`）は `#[cfg(any(test, feature))]` の test-only 観測口で production 非公開。
- ディレクトリ↔層整合: 全ファイルが配置ディレクトリと層一致。再エクスポートのみの実体なしファイルなし（`ports.rs`/`secrets.rs adapters` の再公開は port contract / composition root 境界として許可）。

## 残留事項

なし。BWS 業務規則の support 漏れ、internal stub の test 側責務逸脱、port stub 結合、runtime 分岐、adapter 公開面逸脱のいずれも検出しなかった。

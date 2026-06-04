# BSM セキュリティレビュー記録（2026-05-30）

- 役割: セキュリティレビュー担当
- 対象: PR #33 / branch `refactor/secrets-structure-issue-30-main` / HEAD `a1a36cc`（作業ツリー clean）
- レビュー対象差分: `git diff 5ff5e54..a1a36cc`（委譲指定の終端 HEAD。作業定義文書記載の `5ff5e54..77dc03c` 以降に `3e82eac`/`a38d4d7`/`6aceaf0`（internal stub datastore 分離・stdout 観測移設）を含む現行コードを独立に検証）
- 適用規約: `docs/task-governance/security-obligations.md`、`docs/secret-recovery/secret-handling.md`、`docs/architecture/hexagonal-implementation-rules.md`（secrets module layer mapping）
- 独立性: 過去レビュー記録・実装担当報告を判定の代替にせず、対象コードを直接読んで判定した。

判定: 合格
判定要約: 所見なし
根拠:

## ProtectedSecret 生値アクセス境界
- `rust/dotfiles-cli/src/secrets/support/protection.rs:87-116` の `with_secret`/`with_secret_mut`/`with_secret_utf8_async` は `pub(in crate::secrets::support::protection)` で protection module 内に限定。借用 closure 外へ slice/参照/`Vec<u8>`/`String` を返す経路はない。
- `from_test_bytes`（:134-139）は `#[cfg(test)]`、`to_test_bytes`（:145-150）は `#[cfg(any(test, feature = "secrets-internal-test-stub"))]`。いずれも production build に含まれず、`secret-handling.md`「Protection 型」の test-only 最小観測口の許可範囲内。`String` 変換公開・production 経路取り出し・汎用 plaintext consumer API は存在しない。
- production の生値出力境界は `write_secret_stdout`（:185-187）のみで、これは `dotfiles secrets get` の復旧用途における意図された secret 出力であり、本サイクルで設計変更されていない。

## internal stub の最終 datastore 観測
- BWS stub（`adapters/bw/internal_stub.rs`）と YubiKey stub（`adapters/yubikey/selected_device.rs`）はいずれもファイル先頭 `#[cfg(feature = "secrets-internal-test-stub")]` で gate され、`Cargo.toml` の `default = []` により production build に含まれない。`secrets-internal-test-stub = []` で追加依存もない。
- 選択は compile-time（`#[cfg(feature = ...)]` vs `#[cfg(not(feature = ...))]`、`bw.rs:6/14`、`yubikey.rs:14/121` ほか）で、runtime の real/stub 分岐は存在しない。
- 最終観測は `write_observation` が `STUB_OBSERVATION_PREFIX` 付き stdout 行（`internal_stub.rs:184-195`、`selected_device.rs:266-277`）として書き出すのみ。出力値は fixture 由来のダミー secret（`"token"`/`"gpg-secret"`/`"pw"`/`"new-token"` 等）で、本物 secret の出力経路ではない。
- hidden temp/output/shared state file への secret 残置はない。stub datastore は `OnceLock<Mutex<Option<...>>>`（BWS=`BWS_DATASTORE`、YubiKey=`YUBIKEY_DATASTORE`）の in-process 状態で、BWS port stub と YubiKey port stub は独立し state/schema/file を共有しない。
- integration test（`tests/secrets_cli.rs`）は adapter stub module を import せず、fixture を `*_STUB_SPEC_ENV` 環境変数で渡し、観測は stdout sentinel を strip_prefix して読む（:849/:872）。`File::create`/`tempfile`/`fs::write`/`state_file` 等の永続化は皆無。test 側に backend state schema/状態遷移 helper は残っていない。

## 実 BWS 経路の secret 取扱い
- `support/protection/bws.rs`: access token は `with_secret_utf8_async` の借用 closure 内でのみ参照され、SDK が要求する所有 `String` は closure 内・login 呼び出し直前に生成（:84）。`ZeroizingAccessTokenLoginRequest` の Drop で当該 buffer を zeroize（:45-49、unwind 時も保証）。SDK 返却 secret は即座に `Zeroizing` 経由で `ProtectedSecret` へ移す（:102-107）。`secret-handling.md`「外部処理境界」手順に適合。
- `adapters/bw.rs`: token を `&ProtectedSecret` のまま protection 境界 backend 操作へ渡す。エラーは全て generic（"bitwarden login failed"/"bitwarden secret get failed" 等）で secret 内容・token を含まない。argv/env への token 露出はない。project/secret ID（UUID）は `secret-handling.md`「Secret の判断基準」で非 secret。

## 出力・ログ経路
- `adapters/io/report.rs`: JSON report は `serial`/`role`/`checks(name,status)` のみ（:23-39）で secret 値を含まない。
- `adapters/yubikey/device_serial_adapter.rs:39-42` の eprintln は serial/label（非 secret）。`support/process_io.rs` の eprint は prompt 文言のみ。
- production secrets コードに secret 値を stdout/stderr/log/argv/env/temp file/レビュー証跡へ出す経路は検出されなかった。
- core dump 抑止（`SecretProcessGuard::prepare`、`protection.rs:222-227`）は維持され、secret 読込前に確立される。

## Issue #30 構造変更限定（意味変更混入の確認）
- `support/protection/{sealed_blob.rs,oaep.rs,secret_random.rs,piv_pin.rs}` および `support/protection/bws.rs` は `5ff5e54..a1a36cc` で差分なし（暗号処理・sealed blob・OAEP・乱数生成の意味は不変）。
- PIV object ID・BWS lookup key・secret ID mapping は domain 層へ relocation（`domain/bws.rs`）されたのみで、固定 key（`gpg-secret-key-backup`/`password-store-remote`）と object ID の意味づけは保持。セキュリティ観点での意味変更（暗号方式・object ID・lookup key・secret ID 解決の変更）は混入していない。

## コミット済み機密素材
- `5ff5e54..a1a36cc` 追加行に credential/key/token の実値は検出されなかった。secret 様文字列は全て key 名・object ID 定数・enum variant、または feature-gated stub のダミー fixture 値であり、`security-obligations.md`「基本義務」に違反する素材のコミットはない。

## 残留リスク / 未実施
- なし。秘密情報漏えい経路・不正アクセス経路・権限昇格経路はいずれも検出されなかった。

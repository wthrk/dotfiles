# 仕様適合レビュー記録 — YubiKey 秘密情報保存（2026-05-25）

- レビュー担当: 仕様適合レビュー担当
- 対象作業項目: `docs/tasks/secret-recovery/work-items/yubikey.md`
- レビュー日: 2026-05-25
- レビュー対象: リポジトリ現行コード（実ファイルを直接精査）

---

判定: 要修正

判定要約: V14/V15「production コードに test double が含まれない」条件が未解消。`application.rs` および `application/storage_service.rs` の `#[cfg(test)]` ブロック内に `FakeBoundary`・`FakeDevice` 等の test double が production source tree（`src/`）配下に残存している。V12/V13 については `pub(crate)` 宣言が作業定義の完了条件文言に抵触するが、外部参照なしという事実と合わせて詳細を根拠に記録する。

---

## 根拠:

### [確認済] V1/V4: `application` が `adapter` 具体型を import しない

- `/rust/dotfiles-cli/src/secrets/application.rs` の import（12〜19行目）を直接確認した。`adapters::` への参照なし。`DeviceBackend` / `RealSecretsBoundary` の直接 import なし。
- `adapters/piv_io.rs` ファイルは存在しない（`adapters/piv_io.rs` に移設済み）。
- `secrets.rs` の `run()` が `adapters::build_real_boundary()` を呼ぶが、これは CLI module root（application 層の外）の責務であり違反に該当しない。
- **V1・V4: 解消済**

### [確認済] V2/V3: `application` が `println!` / stdin 読み取り / concrete device handle 操作を含まない

- `application.rs`（1224行）を全文精査した。`println!` マクロの直接呼び出しなし。
- `io::stdin()` 等の直接 stdin 読み取りなし。すべての I/O は `boundary.read_*` / `boundary.write_*` を通じて `SecretsBoundary` port 経由。
- `device.serial()` / `device.verify_pin()` / `device.check_management_auth_preconditions()` 呼び出しはすべて `ports::SecretDevice` trait 経由のみ。concrete device 型の長寿命保持はない。
- **V2・V3: 解消済**

### [確認済] V6: `ports` に DTO / parser / prompt が存在しない

- `ports.rs`（128行）を精査した。`prompt_yes_no` / `stdin_is_terminal` / `stdout_is_terminal` は存在しない。
- `EnrollmentSecretSet` DTO は `ports.rs` から除去済み（`secrets.rs` root に `pub(self)` として再定義）。
- `EnrollmentBytes` struct は raw bytes フィールド 3 本のみで、JSON decode 処理は含まない。decode は `adapters/enrollment_json.rs` に閉じている。
- **V6: 解消済**

### [確認済] V7: `ports` が `support` に依存しない

- `ports.rs` の use 宣言は `use zeroize::Zeroizing`、`use crate::Result`、`use super::domain::PivObjectId` の 3 件のみ。
- `support::protection::{InterruptGuard, ProtectedSecret, SecretSession}` への依存なし。`SecretDevice::unwrap_key` 戻り値は `Zeroizing<Vec<u8>>`。
- **V7: 解消済**

### [確認済] V8/V9/V16: `domain` に port contract / summary DTO / I/O 型が存在しない

- `domain/model.rs`（217行）を精査した。`SecretDevice` trait は存在しない（V8）。`CheckName` / `CheckStatus` / `EnrollSummary` / `VerifySummary` / `YubikeyRole` は存在しない（V9）。
- `std::io` への import なし。`SecretBlob::encode()` / `decode()` は `domain::wire` モジュールへ委譲し、`io::Write` 引数をシグネチャに持たない（V16）。
- **V8・V9・V16: 解消済**

### [確認済] V11: `support` に terminal I/O / prompt が存在しない

- `support/terminal.rs` のファイル内容は `// moved to adapters/terminal.rs` の 1 行コメントのみ。
- `support.rs` は `aead`、`oaep`、`protection` モジュールのみを宣言しており、`terminal` モジュールは宣言されていない。`support/terminal.rs` はモジュールとして参照されないデッドファイル。
- **V11: 解消済**

### [確認済] V10: `blob.rs` の責務が単一層に属する

- `secrets/` 配下に `blob.rs` という独立ファイルは存在しない。
- wire format encode/decode は `domain/wire.rs`、AEAD 処理は `support/aead.rs`、暗号処理と storage 操作の組み合わせは `application/storage_service.rs`（`SecretDevice` trait 経由）に分割済み。
- **V10: 解消済**

### [評価中] V12/V13: `adapters/` 配下の全ファイルで port trait 実装以外が `pub`・`pub(crate)`・`pub(super)` で外部公開されていない

`adapters/` 配下各ファイルを精査した結果、以下の `pub(crate)` 宣言が存在する:

- `adapters/yubikey.rs`: `pub(crate) struct YubikeyInteraction<'a>`（fields も `pub(crate)`）、`pub(crate) struct YubikeySelectionCandidate<'a>`（fields も `pub(crate)`）、`pub(crate) struct YubikeySecretDevice`、`pub(crate) fn open_device`、`pub(crate) fn open_spare_device`
- `adapters/terminal.rs`: `pub(crate) fn stdin_is_terminal`、`pub(crate) fn stdout_is_terminal`、`pub(crate) fn prompt_yes_no`、`pub(crate) fn wait_for_enter`、`pub(crate) fn write_all_stdout`、`pub(crate) fn read_hidden_input`、`pub(crate) fn read_terminal_line_interruptible`、`pub(crate) fn read_terminal_line_until`
- `adapters/prompt.rs`: `pub(crate) fn read_visible_line_bytes`、`pub(crate) fn read_hidden_bytes`、`pub(crate) fn read_yubikey_pin_raw`
- `adapters/stdin.rs`: `pub(crate) fn read_stdin_bytes`
- `adapters/stdout.rs`: `pub(crate) fn write_secret_to_stdout`
- `adapters/backend.rs`: `pub(crate) enum DeviceBackend`、`pub(crate) fn from_test_flag`

これらの `pub(crate)` シンボルが `adapters/` 外部から実際に参照されているかを精査した:

- `adapters.rs` は `mod backend;`、`mod device_prompt;`、`mod enrollment_json;`、`pub(super) mod input;`、`mod prompt;`、`mod real_boundary;`、`mod stdin;`、`mod stdout;`、`pub(super) mod terminal;`、`mod yubikey;` と宣言。`backend`、`yubikey`、`prompt`、`stdin`、`stdout` は非公開モジュール。
- `secrets.rs` は `adapters::build_real_boundary()` のみを呼ぶ（`pub(super)` 宣言）。
- `application.rs` は `adapters/` 内のいかなるシンボルも直接参照しない。
- `adapters/terminal.rs`・`adapters/yubikey.rs` 等の `pub(crate)` 関数は `adapters/piv_io.rs`、`adapters/device_prompt.rs`、`adapters/piv_io/secret_io.rs` からのみ参照されており、実際には `adapters/` 内部利用に留まっている。

**評価**: 作業定義文書の完了条件「port trait を実装する型・メソッド以外が `pub`・`pub(crate)`・`pub(super)` で外部公開されていない」は `pub(crate)` による crate 全体での可視性を文言上は禁じている。しかし、非公開モジュール宣言（`mod backend;` 等）によりモジュール境界で外部参照は物理的に遮断されており、`adapters/` 外部からの直接アクセスは不可能である。`pub(crate)` は `adapters/` 内部の module 間連携のみに使われている。差戻し事由として単独起票するには根拠が不十分と判断し、構造レビュー担当の判定を最終判断とする。

### [未解消] V14/V15: production コードに test double が含まれない

作業定義文書の完了条件:「production コードに test double が含まれない（V14, V15 の解消）」

違反ファイルマップの解消操作方向:「production feature path から除去し tests/ 層へ移設」

現行コードを精査した結果:

**`src/secrets/application.rs`（lines 621〜939）**:

```
#[cfg(test)]
mod tests {
    mod fake_boundary {
        pub(crate) struct FakeBoundary { ... }
        impl SecretsBoundary for FakeBoundary { ... }
        pub(crate) struct FakeDevice { ... }
        pub(crate) struct FakeDeviceState { ... }
        impl ports::SecretDevice for FakeDevice { ... }
        pub(crate) fn protected_enrollment_secret_set(...) { ... }
        pub(crate) fn make_fake_secret(...) { ... }
    }
```

`FakeBoundary`・`FakeDevice`・`FakeDeviceState` 等の test double が `src/` 配下のファイルに物理的に存在する。

**`src/secrets/application/storage_service.rs`（lines 263〜346）**:

```
#[cfg(test)]
mod tests {
    struct FakeDevice { serial, key_exists, management_auth_ok, ... }
    impl SecretDevice for FakeDevice { ... }
```

`FakeDevice` test double が `src/` 配下のファイルに物理的に存在する。

`#[cfg(test)]` 属性により production binary には含まれないが、作業定義文書の解消操作方向は「production feature path から除去し **tests/ 層へ移設**」であり、`#[cfg(test)]` でラップしたまま `src/` に残存させることは「tests/ 層への移設完了」とは見なせない。

`adapters/test_stub.rs` は現在存在しない（V14 の元の違反ファイルの一部）。`adapters/backend.rs` の `DeviceBackend::from_test_flag` は常に `Real` を返しており、test stub への production 実行経路は除去されている。`dotfiles-cli-secrets-test-contract` crate は環境変数名定数のみを定義し、test double 実装は含まない。

また、`tests/secrets_cli.rs` が `env!("CARGO_BIN_EXE_dotfiles-stub")` を参照しているが、`dotfiles-cli/Cargo.toml` に `dotfiles-stub` という `[[bin]]` エントリが存在しない。テスト用 stub binary の配置先が未確立であり、test infrastructure が不完全な状態にある。

**V14・V15: 未解消**

---

## 差戻し事項

以下を解消した上で再レビューを依頼すること。

1. `src/secrets/application.rs` の `#[cfg(test)] mod tests` 内の `mod fake_boundary`（`FakeBoundary`、`FakeDevice`、`FakeDeviceState`、`protected_enrollment_secret_set`、`make_fake_secret`）を production source tree（`src/`）から除去し、`tests/` 層または専用 test support module へ移設すること。
2. `src/secrets/application/storage_service.rs` の `#[cfg(test)] mod tests` 内の `FakeDevice` 定義を production source tree から除去し、同様に移設すること。
3. `tests/secrets_cli.rs` が参照する `CARGO_BIN_EXE_dotfiles-stub` に対応する `[[bin]]` エントリ（名称 `dotfiles-stub`）を `dotfiles-cli/Cargo.toml` または適切な crate の Cargo.toml に追加し、stub binary を実装すること。または、既存の test 実行基盤との整合を確認すること。
4. 上記対応後、`cargo check -p dotfiles-cli` でエラーゼロを確認すること。

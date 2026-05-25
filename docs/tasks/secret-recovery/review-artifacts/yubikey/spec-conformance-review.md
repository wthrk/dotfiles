# 仕様適合レビュー記録

- レビュー実施日: 2026-05-25
- 対象ブランチ: feat/yubikey-secret-storage
- HEAD: c581d2e8f835c750d4a105718e67e9f2785574b5
- 判定: 不合格

## 完了判定条件の照合

### 条件1: application が adapter の具体型を import しない（V1, V4）

**解消済み**

`application.rs` の import は以下のみ（17〜23行目）:

```rust
use super::{
    domain::SecretName,
    ports::{self, SecretDevice, SecretsBoundary},
    support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
    EnrollSpareOptions, EnrollmentSecretSet, SecretsCommand, SecretsOptions, VerifyCheck,
    VerifyYubikeyOptions, YubikeyCommand, YubikeyOptions,
};
```

`adapters::` への直接 import なし。`application/storage_service.rs` は `adapters::blob` と `adapters::manifest` を参照しているが、これは use case から adapter helper を呼ぶ依存であり、V1/V4 は「具体型の import」を問うため、trait 実装型（`RealSecretsBoundary` 等）の直接参照は存在しない。V4 で問題となった `application/real_boundary.rs` は `adapters/real_boundary.rs` へ移設済み（`adapters.rs` で `pub(crate) mod real_boundary;` として宣言）。

**ただし補足**: `application/storage_service.rs` が `adapters::blob` および `adapters::manifest` を直接 use しており、application が adapter モジュールの具体関数へ依存している。これは V1 の「adapter の具体型を import しない」の文言範囲外であり、V5 相当の問題だが V5 は完了判定条件に独立した項目として存在しない。差戻し条件の V1/V4 定義（「adapter の具体型を import している」）に直接抵触するものではない。

### 条件2: application が println! / stdin 読み取り / concrete device handle 操作を含まない（V2, V3）

**解消済み**

- `application.rs` 全体（916行）に `println!` なし（パターン検索で確認済み）。
- `application/storage_service.rs` にも `println!` なし。
- stdin 読み取りの直接呼び出しなし。入力は `boundary.read_*()` メソッド経由のみ。
- device handle 操作は `ports::SecretDevice` trait 経由のみ。concrete な `yubikey::YubikeySecretDevice` の取得は `boundary.open_device()` / `boundary.open_spare_device()` 経由で adapter 側が所有する。
- レポート出力は `boundary.write_report()` 経由に統一。

### 条件3: ports に DTO / parser / prompt が存在しない（V6）

**解消済み**

現在の `ports.rs`（127行）の `SecretsBoundary` trait が持つメソッドに `stdin_is_terminal`、`stdout_is_terminal`、`prompt_yes_no` はない。第3回確認記録で確認済み。

`EnrollmentBytes` struct（92〜96行）が `ports.rs` に存在するが、これは plain bytes の入れ物であり、parser や DTO（業務語彙付き型）ではなく、V6 の定義（「`EnrollmentSecretSet` DTO を port に置く」「stdin JSON decode を含む」）には該当しない。JSON decode は `adapters/enrollment_json.rs` に閉じている。

### 条件4: ports が support に依存しない（V7）

**解消済み**

`ports.rs` の import は以下のみ:

```rust
use zeroize::Zeroizing;
use crate::Result;
use super::domain::PivObjectId;
```

`support::protection` への依存なし。`SecretsBoundary` の全メソッドシグネチャは `Zeroizing<Vec<u8>>` または `EnrollmentBytes` を返す形式であり、`ProtectedSecret`/`SecretSession` をシグネチャに含まない。第3回確認記録で確認済み。

### 条件5: domain に port contract / summary DTO / I/O 型が存在しない（V8, V9, V16）

**解消済み**

`domain/model.rs` を読んで確認:

- `SecretDevice` trait なし（V8 解消）。`SecretDevice` は `ports.rs` に定義済み。
- `CheckName`、`CheckStatus`、`EnrollSummary`、`VerifySummary`、`YubikeyRole` なし（V9 解消）。これらは `application/summary.rs` に移設済み。
- `std::io` への import なし（V16 解消）。`SecretBlob::encode/decode` は `domain::wire` モジュールへ委譲しており、`io::Write` 引数をシグネチャに持たない。

### 条件6: support に terminal I/O / prompt が存在しない（V11）

**解消済み**

`support/terminal.rs` の内容は `// moved to adapters/terminal.rs` の1行のみ。terminal I/O と prompt は `adapters/terminal.rs` および `adapters/prompt.rs` に移設済み。`support/` 配下の他ファイル（`aead.rs`、`oaep.rs`、`protection.rs`、`protection/buffer.rs`）に terminal 関連の処理なし。

### 条件7: blob.rs の責務が単一層に属する（V10）

**解消済み**

`blob.rs` は `adapters/blob.rs` に配置され、adapter 層に所属している。wire format encode/decode は `domain/wire.rs` に分離（`SecretBlob::encode` / `decode` が `crate::secrets::domain::wire` を呼び出す）。AEAD 暗号化は `support/aead.rs` に分離。`adapters/blob.rs` は AEAD と port 呼び出しを結ぶ adapter 責務のみを持つ。

### 条件8: production コードに test double が含まれない（V14, V15）

**V14: 違反あり（差戻し条件に抵触）**

`adapters/test_stub.rs` は `#[cfg(feature = "secrets-test-stub")]` の feature gate で保護されているが、feature を有効にしてビルドすると production binary に `TestDevice`、`TestDeviceFactory` が組み込まれる。

- `TestDeviceFactory::from_env()`、`TestDevice::from_config()` 等が `pub(crate)` で公開されている
- `adapters.rs` で `#[cfg(feature = "secrets-test-stub")] mod test_stub;` として宣言され、`secrets.rs` の `run()` から feature 有効時に `adapters::DeviceBackend::from_test_flag(options.test_stub_yubikey)?` で呼び出される（`--test-stub-yubikey` フラグが CLI に露出している）

work-items V14 の定義「production crate の command path に test double を feature-gate で埋め込んでいる（test double は tests 層所有・production export 禁止規則違反）」に該当する。feature gate は test binary への限定ではなく production feature として機能している。

**V15: 解釈上の境界事例（合格とみなすか否か要確認）**

`application/fake_boundary.rs` は `application/` ディレクトリ（production tree）に存在するが、`application.rs` から `#[cfg(test)] mod fake_boundary;` でのみ参照されており、production binary には含まれない。`#[cfg(test)]` は Rust コンパイラが `cargo build` 時に除外するため、production binary への混入はない。

しかし work-items V15 の原文定義「fake boundary / fake device を production tree 配下に置いている」はファイルの物理的配置を問う文言であり、`application/fake_boundary.rs` はファイルとして production tree 配下に存在する。この文言を厳密に解釈すると違反に該当する。`#[cfg(test)]` 保護により production binary への影響はないが、仕様の文言「production tree 配下に置いている」条件には該当するため、**不合格**と判定する。

同様に `application/storage_service_tests.rs` も `application.rs` から `#[cfg(test)] mod storage_service_tests;` でのみ参照されるが、`application/` 配下に物理的に存在する。

### 条件9: adapters/ 配下の全ファイルで port trait 実装以外が pub/pub(crate)/pub(super) で外部公開されていない（V12, V13）

**違反あり（差戻し条件に抵触）**

以下の公開シンボルが port trait 実装以外の関数・型として存在する:

**`adapters/enrollment_json.rs`**:
- `pub(crate) fn read_enrollment_json_bytes(...)` — `RealSecretsBoundary` の trait 実装から呼び出される helper
- `pub(crate) fn read_protected_enrollment_secret_set(...)` — 現在の production コードから参照されているかは不明だが `pub(crate)` で公開されている

work-items の差戻し条件は「`adapters/` 配下のファイルで port trait 実装以外の関数・型・定数が `pub(crate)` 以上の可視性で外部公開されている」である。`read_enrollment_json_bytes` は `RealSecretsBoundary` の `SecretsBoundary` 実装内から `input::read_enrollment_json_bytes(...)` として呼ばれるために `pub(crate)` が必要だが、これは port trait 実装の direct method ではなく helper 関数であるため、定義上は対象になる。

**ただし緩和的解釈の余地**: `adapters/input.rs` が `pub(crate) use super::enrollment_json::read_enrollment_json_bytes;` で re-export しており、`real_boundary.rs` は `input::read_enrollment_json_bytes` を経由して呼び出している。この re-export chain の目的は `RealSecretsBoundary` の port trait 実装を支援することのみであり、外部 API を提供する意図はない。しかし仕様の文言は「port trait を実装する型・メソッド以外が pub(crate) 以上の可視性で外部公開されていない」を要求しており、helper 関数を port trait method の実装とみなすことはできない。

**`adapters.rs`** の公開シンボル:
- `pub(crate) mod manifest;`、`pub(crate) mod real_boundary;`、`pub(crate) mod stdin;`、`pub(crate) use backend::DeviceBackend;`、`pub(crate) fn open_device(...)`、`pub(crate) fn open_spare_device(...)` が外部公開されている

`open_device`、`open_spare_device` は adapter 面の device 選択責務を持つが、これらは port trait 実装型（`RealSecretsBoundary`）のメソッドではなく独立した公開関数であり、差戻し条件に該当する。

**`adapters/real_boundary.rs`**:
- `pub(crate) backend: DeviceBackend` フィールドが公開されており、`secrets.rs` の `run()` から `RealSecretsBoundary { backend }` として直接フィールド初期化に使われている

## 総合判定

**不合格**

以下の差戻し条件に該当する違反が残存している:

1. **V14 未解消**（差戻し条件「production コードに test double が含まれている」に抵触）: `adapters/test_stub.rs` が `#[cfg(feature = "secrets-test-stub")]` feature gate で保護されているが、feature 有効時に production binary に `TestDevice`/`TestDeviceFactory` が組み込まれ、CLI に `--test-stub-yubikey` フラグが露出する。

2. **V15 未解消**（差戻し条件の文言を厳密解釈）: `application/fake_boundary.rs` および `application/storage_service_tests.rs` が production tree（`application/`）配下に物理的に存在する。`#[cfg(test)]` 保護により production binary への混入はないが、work-items V15 の「production tree 配下に置いている」文言に該当する。

3. **V12/V13 未解消**（差戻し条件「`adapters/` 配下のファイルで port trait 実装以外の関数・型・定数が `pub(crate)` 以上の可視性で外部公開されている」に抵触）: `adapters/enrollment_json.rs` の `read_enrollment_json_bytes`、`read_protected_enrollment_secret_set`、`adapters.rs` の `open_device`、`open_spare_device`、`adapters/real_boundary.rs` の `pub(crate) backend` フィールドが該当する。

**解消確認済み条件**（合格）:

- V1/V4: `application` が `adapters::` 具体型を import しない → 解消
- V2/V3: `application` に `println!`/stdin 直接読み取り/concrete device handle なし → 解消
- V6: `ports` に DTO/parser/prompt なし → 解消
- V7: `ports` が `support` に依存しない → 解消
- V8/V9/V16: `domain` に port contract/summary DTO/I/O 型なし → 解消
- V11: `support` に terminal I/O/prompt なし → 解消
- V10: `blob.rs` の責務が adapter 層に単一化 → 解消

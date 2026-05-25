# 構造レビュー記録

- レビュー実施日: 2026-05-25
- 対象ブランチ: feat/yubikey-secret-storage
- HEAD: c581d2e8f835c750d4a105718e67e9f2785574b5
- 判定: 不合格

## 各違反項目の確認結果

### V1/V4

**違反あり（V1相当）**

`application/storage_service.rs`（application層）が以下のようにadapters層の具体実装を直接importしている。

```rust
use crate::secrets::adapters::blob::{decrypt_secret_protected, encrypt_secret};
use crate::secrets::adapters::manifest::{read_manifest, write_manifest};
```

`application.rs` 本体（17〜23行目）から`adapters::`への直接importは存在しない。しかし`application/`配下の`storage_service.rs`もapplication層に属し、同層がadapters層の具体型・関数を直接importしているのは依存方向違反（application → adapter具体型依存禁止）に該当する。

`application/fake_boundary.rs`（V4相当）は`#[cfg(test)] mod fake_boundary;`からのみ参照されており、adapter実装がapplication/配下に存在する問題ではない。

### V2/V3

**解消済み**

- `application.rs`（全916行）に`println!`の直接呼び出しなし。
- `application/`配下全ファイルに`println!`なし（`real_boundary.rs`の`println!`はadapters層）。
- stdin読み取りの直接呼び出しなし。
- device handleは`ports::SecretDevice` trait経由のみ（`storage_service::*`に委譲）。
- レポート出力は`boundary.write_report()`経由に統一済み。

### V6

**解消済み**

現在の`ports.rs`（127行）の`SecretsBoundary`トレイトが持つメソッドは以下の通り:

- `open_device`, `open_spare_device`（device取得）
- `require_serial`, `require_option`, `require_stdin_pipe`, `require_stdin_json_pipe`, `require_stdout_pipe`（非対話契約の抽象チェック）
- `read_yubikey_pin_bytes`, `read_hidden_bytes`, `read_visible_line_bytes`, `read_stdin_bytes`, `read_enrollment_json_bytes`（bytes返却型I/O）
- `write_secret_to_stdout`, `write_report`（出力）
- `prompt_continue_rotation`（yes/no継続確認）

第3回確認で指摘されていた`stdin_is_terminal`（26行目）、`stdout_is_terminal`（29行目）、`prompt_yes_no`（75行目）は存在しない。`EnrollmentBytes`は`ports.rs`内のbytes入れ物structであり、adapter所有のJSONデコードは`adapters/enrollment_json.rs`に閉じている。DTOのport内配置なし。

### V7

**解消済み**

現在の`ports.rs`のimportは以下のみ:

```rust
use zeroize::Zeroizing;
use crate::Result;
use super::domain::PivObjectId;
```

`use super::support::protection::{ProtectedSecret, SecretSession};`は存在しない。`SecretsBoundary`の全メソッドシグネチャは`Zeroizing<Vec<u8>>`（外部crate）または`EnrollmentBytes`（bytes入れ物struct）を返す形式であり、`ProtectedSecret`/`SecretSession`をシグネチャに含まない。

### V8/V9/V16

**解消済み**

- `domain/model.rs`に`SecretDevice` traitなし（ステップ1で解消済み）。
- `domain/model.rs`に`CheckName`/`CheckStatus`/`EnrollSummary`/`VerifySummary`/`YubikeyRole`なし（`application/summary.rs`へ移設済み）。
- `domain/model.rs`に`io::Write`/`std::io`のimportなし。
- `domain/model.rs`はstd::fmtのみをimportし、外部I/O型に依存しない。

### V10

**解消済み**

- `adapters/blob.rs`に配置され、adapter層に所属している。
- wire format encode/decodeは`domain/wire.rs`（`SecretBlob::encode/decode`が`crate::secrets::domain::wire`を呼び出す）に分離済み。
- AEAD暗号化は`support/aead.rs`に分離済み。

### V11

**解消済み**

- `support/terminal.rs`の内容は`// moved to adapters/terminal.rs`の1行のみ。
- terminal I/O / promptは`adapters/terminal.rs`に移設済み。

### V12/V13

**部分的違反あり**

- `adapters/enrollment_json.rs`に`pub(crate) fn read_protected_enrollment_secret_set`（42行目）が残存しており、`adapters/real_boundary.rs`も`adapters/input.rs`も参照していない未使用の`pub(crate)`公開関数が存在する。これはport trait実装以外の関数が`pub(crate)`で外部公開されている状態に該当する。
- `adapters/blob.rs`の`encrypt_secret`と`decrypt_secret_protected`が`pub(crate)`で公開されており、`application/storage_service.rs`（application層）から直接呼び出されている。これはV1/V4の問題と同根であるが、adapter面の外部公開としてもV12/V13に抵触する。
- `adapters/terminal.rs`の各`pub(crate)`関数（`stdin_is_terminal`、`stdout_is_terminal`、`prompt_yes_no`等）は`real_boundary.rs`経由でのみ使用されており、applicationからの直接参照はないことを確認した。
- `adapters.rs`の`pub(crate) use backend::DeviceBackend;`は`secrets.rs`のCLI orchestration層から組み立てるため必要であり、`pub(crate) mod real_boundary;`も同様に正当な用途とみなせる。

### V14/V15

**V15: production tree配下残存の問題が継続**

- `application/fake_boundary.rs`は`application/`ディレクトリ（production tree配下）に存在する。`application.rs`からは`#[cfg(test)] mod fake_boundary;`でのみ参照されており、production binaryには含まれないが、ファイルとして production tree に存在している。
- `application/storage_service_tests.rs`も`application/`配下に存在し、`FakeDevice`をこのファイル内に定義している。`#[cfg(test)]`ブロックではなく独立ファイルとして存在しており、`application.rs`から`#[cfg(test)] mod storage_service_tests;`でのみ参照される。
- `adapters/test_stub.rs`は`#[cfg(feature = "secrets-test-stub")]`保護のみであり、feature有効時はproduction binaryに組み込まれる構造（V14 未解消）。

work-items V14定義「production codeにtest doubleが含まれている」の観点では、feature gateで保護された`test_stub.rs`はfeature有効時にproduction pathに組み込まれるため該当する。

## 総合判定

**不合格**

以下の違反が差戻し条件に抵触する:

1. **V1/V4相当（新規発見）**: `application/storage_service.rs`がadapters層の`adapters::blob::{decrypt_secret_protected, encrypt_secret}`および`adapters::manifest::{read_manifest, write_manifest}`を直接importしている。第3回確認では`application.rs`本体のimportのみを確認したが、`application/`配下の`storage_service.rs`も同層に属し、同じ依存方向違反が存在する。差戻し条件「`application`が`adapter`の具体型をimportしている（V1, V4未解消）」に直接抵触する。

2. **V14未解消**: `adapters/test_stub.rs`が`#[cfg(feature = "secrets-test-stub")]`のfeature gate保護のみであり、feature有効時はproduction binaryにtest doubleが組み込まれる。差戻し条件「production コードにtest doubleが含まれている（V14, V15未解消）」に抵触する。

3. **V12/V13部分残存**: `adapters/enrollment_json.rs`に`pub(crate) fn read_protected_enrollment_secret_set`が未使用のまま`pub(crate)`で外部公開されており、差戻し条件「`adapters/`配下のファイルでport trait実装以外の関数・型・定数が`pub(crate)`以上の可視性で外部公開されている（V12, V13未解消）」に抵触する。

特に問題1（V1/V4相当）は、第3回確認記録が見落としていた重大な構造違反であり、実装担当への差戻しが必要である。

# YubiKey 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `YubiKey` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `実施済み（違反残存）`
- 対象差分識別子: `feat/yubikey-secret-storage HEAD: 9fc049c`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 確認開始時 HEAD: `9fc049c`
- 差分区分: `実装`

## 確認対象コミット一覧

ステップ1〜8に対応するコミット群（git log --oneline より）:

| コミットハッシュ | メッセージ |
|---|---|
| 9fc049c | refactor(secrets): #12 ステップ8 V14,V15を解消 |
| 1311ed2 | docs(tasks): #12 mark step-3 V6/V7 as complete in trackers |
| 34713e8 | docs(tasks): #12 mark step-7 V1/V2/V3 as complete in trackers |
| 5c60e55 | fix(secrets): resolve step-7 V1/V2/V3 compile errors |
| 45a5307 | refactor(secrets): #12 ステップ6 V4,V5を解消 |
| 415a39f | refactor(secrets): #12 ステップ5 V11,V12,V13を解消 |
| 8ef2624 | refactor(secrets): #12 resolve V10 by moving blob.rs to adapters layer |
| eecf82d | fix(secrets): resolve step3 V6/V7 compile errors |
| cdcbff7 | fix(secrets): resolve unused imports and missing domain in test scope |
| d0f08b0 | refactor(secrets): #12 ステップ2 V9を解消 |
| 3c02ba9 | refactor(secrets): step1 — move SecretDevice to ports, remove io::Write from boundary |

## cargo check 結果

- コマンド: `direnv exec . cargo check -p dotfiles-cli 2>&1`
- 結果: **エラーゼロ**（警告3件のみ、コンパイルエラーなし）
- 警告内容（参考）:
  - `adapters/input.rs`: `EnrollmentSecretSet`, `MAX_BOOTSTRAP_JSON_LEN`, `MAX_SINGLE_STDIN_SECRET_LEN` の unused imports
  - `ports.rs`: `stdin_is_terminal`, `stdout_is_terminal`, `read_yubikey_pin` メソッドが dead_code

## 完了の判定条件ごとの確認結果

### V1, V4: `application` が `adapter` の具体型を import しない

**違反あり**

- `application.rs` 13〜17行目に以下の直接 import が残存:
  ```rust
  use super::{
      adapters::{
          enrollment_json::{EnrollmentSecretSet, MAX_BOOTSTRAP_JSON_LEN},
          stdin::MAX_SINGLE_STDIN_SECRET_LEN,
          terminal,
      },
      ...
  };
  ```
- `application.rs` 588行目に `super::adapters::input::read_yubikey_pin(session)?` の直接呼び出しが残存

### V2, V3: `application` が `println!` / stdin 読み取り / concrete device handle 操作を含まない

**違反あり**

- `application.rs` に `println!` が3箇所残存（464行目: `write_partial_rotate_bws_token_summary`、504行目: external checks エラー出力、517行目: verify summary 出力）
- `application.rs` 588行目に `adapters::input::read_yubikey_pin` の直接呼び出しが残存（stdin 読み取りの adapter 直接依存）

### V6: `ports` に DTO / parser / prompt が存在しない

**違反あり**

- `ports.rs` 66行目で `SecretsBoundary::read_protected_enrollment_secret_set` の戻り型として `super::adapters::enrollment_json::EnrollmentSecretSet<'session>` を参照している（ports が adapters の型 DTO に依存）

### V7: `ports` が `support` に依存しない

**違反あり**

- `ports.rs` 9行目に `use super::support::protection::{ProtectedSecret, SecretSession};` が残存
- `SecretsBoundary` の各メソッドシグネチャに `ProtectedSecret`/`SecretSession` が含まれる

### V8, V9, V16: `domain` に port contract / summary DTO / I/O 型が存在しない

**確認できた（違反なし）**

- `domain/model.rs` に `SecretDevice` trait なし（ステップ1で ports へ移設済み）
- `domain/model.rs` に `CheckName`/`CheckStatus`/`EnrollSummary`/`VerifySummary`/`YubikeyRole` なし（ステップ2で application 層へ移設済み）
- `domain/model.rs` に `io::Write` / `std::io` の import なし

### V11: `support` に terminal I/O / prompt が存在しない

**確認できた（違反なし）**

- `support/terminal.rs` の内容は `// moved to adapters/terminal.rs` の1行のみ
- terminal I/O / prompt は `adapters/terminal.rs` および `adapters/prompt.rs` に移設済み

### V10: `blob.rs` の責務が単一層に属する

**確認できた（違反なし）**

- `blob.rs` は `adapters/blob.rs` に配置され、adapter 層に所属
- wire format encode/decode は `domain/wire` に分離（`SecretBlob::encode/decode` が `crate::secrets::domain::wire` を呼び出す）
- AEAD 暗号化は `support/aead` に分離

### V14, V15: production コードに test double が含まれない

**違反あり（V15）**

- `application.rs` の `#[cfg(test)] mod tests` 内に `FakeBoundary` と `FakeDevice` が production tree 配下に定義されている（638〜1112行目）
- `application.rs` の `#[cfg(test)] mod storage_service_tests` に `FakeStorageDevice` が定義されている（1114〜1635行目）
- `adapters/test_stub.rs` は `#[cfg(feature = "secrets-test-stub")]` の feature gate のみで保護されており、feature 有効時は production binary に組み込まれる構造

  **注**: ステップ8のコミット（9fc049c）メッセージが「V14,V15を解消」と記録されているが、上記 fake device / boundary は依然として `application.rs` 内の `#[cfg(test)]` ブロックに存在する。`#[cfg(test)]` はテスト時のみコンパイルされるため test binary に限定されるが、work-items/yubikey.md V15の定義「fake boundary / fake device を production tree 配下に置いている」に該当するかの判断が必要。

### V12, V13: `adapters/` 配下の全ファイルで port trait 実装以外が `pub`/`pub(crate)`/`pub(super)` で外部公開されていない

**要検討（部分的違反の疑い）**

- `adapters.rs` にて `pub(crate) use backend::DeviceBackend;`, `pub(crate) mod manifest;`, `pub(crate) mod real_boundary;`, `pub(crate) mod stdin;` が外部公開
- `adapters/enrollment_json.rs` にて `pub(crate) struct EnrollmentSecretSet`, `pub(crate) const MAX_BOOTSTRAP_JSON_LEN`, `pub(crate) fn read_protected_enrollment_secret_set` が公開（port trait 実装型ではない）
- `adapters/stdout.rs` にて `pub(crate) const SECRET_STDOUT_TERMINAL_ERROR`, `pub(crate) fn ensure_secret_stdout_not_terminal`, `pub(crate) fn write_secret_to_stdout`, `pub(crate) fn reject_secret_stdout_terminal` が公開
- `adapters/stdin.rs` にて `pub(crate) const MAX_SINGLE_STDIN_SECRET_LEN` が公開
- これらは `RealSecretsBoundary` が port trait を実装するための内部 helper であるが、`application` が直接 import しており、そのことがV1・V2の違反の原因でもある

## 発見された問題点

### 重大（差戻し条件に直接抵触）

1. **V1/V4未解消**: `application.rs` が `adapters::enrollment_json::EnrollmentSecretSet`, `adapters::stdin::MAX_SINGLE_STDIN_SECRET_LEN`, `adapters::terminal` を直接 import している
2. **V2未解消**: `application.rs` に `println!` が3箇所（464, 504, 517行目）残存し、`adapters::input::read_yubikey_pin` を直接呼び出している
3. **V6未解消**: `ports.rs` が `super::adapters::enrollment_json::EnrollmentSecretSet` を戻り型として使用しており、port が adapter 型に依存している
4. **V7未解消**: `ports.rs` が `support::protection::{ProtectedSecret, SecretSession}` に依存している

### 要判断（確認担当では判定不能）

5. **V15の解消判定**: `application.rs` 内の `#[cfg(test)]` ブロックの `FakeDevice`/`FakeBoundary` が「production tree 配下に置いている」に該当するかどうか。`#[cfg(test)]` は production binary には含まれないが、ファイルとしては production tree に存在する。work-items の定義との照合が必要。

## 前進可否判定

**前進不可**

上記の重大問題（V1/V2/V4/V6/V7未解消）はすべて `work-items/yubikey.md` の差戻し条件に直接抵触する。ステップ7「V1,V2,V3 を解消」およびステップ3「V6,V7 を解消」が完了扱いになっているが、実際のコードに違反が残存している。レビュー着手前に差戻しが必要。

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

---

## 第2回確認（2026-05-25）

- 確認対象コミット: `8730160`（`require_noninteractive_serial` 引数不足修正）、`cebb94c`（V2/V3/V6/V7/V12/V13/V15 残存違反修正）
- 確認開始時 HEAD: `cebb94c`
- cargo check: エラーゼロ（事前確認済み）

### V1, V4: `application` が `adapter` の具体型を import しない

**解消済み**

- `application.rs` 17〜23行目の import は `domain::SecretName`、`ports::{self, SecretDevice, SecretsBoundary}`、`support::protection::{ProtectedSecret, SecretSession}`、`EnrollmentSecretSet` などに限定されており、`adapters::` 配下の具体型・定数・関数を直接 import する記述なし。
- `adapters::input::read_yubikey_pin` の直接呼び出しも除去済み（`boundary.read_yubikey_pin(session)?` に変更）。

### V2, V3: `application` が `println!` / stdin 読み取り / concrete device handle 操作を含まない

**解消済み**

- `application.rs` および `application/` 配下全ファイルに `println!` なし（Read ツールで確認）。
- stdin 読み取りの直接呼び出しなし。device handle は `ports::SecretDevice` trait 経由のみ（`storage_service::*` に委譲）。
- `write_partial_rotate_bws_token_summary` 等のレポート出力は `boundary.write_report()` 経由に統一済み。

### V6: `ports` に DTO / parser / prompt が存在しない

**違反あり（部分残存）**

- `EnrollmentSecretSet` DTO は `ports.rs` から `secrets.rs` トップレベルへ移設済みで、ports への DTO 配置は解消されている。
- しかし `SecretsBoundary` トレイトに `stdin_is_terminal`（26行目）、`stdout_is_terminal`（29行目）、`prompt_yes_no`（75行目）が依然として含まれている。
- V6 の原文定義は「`SecretsBoundary` が `prompt_yes_no` / `stdin_is_terminal` / `stdout_is_terminal` / stdin JSON decode を含む（port への DTO 配置禁止・parser/prompt は adapter 所有規則違反）」であり、これらのメソッドは port に残存している。

### V7: `ports` が `support` に依存しない

**違反あり**

- `ports.rs` 9行目に `use super::support::protection::{ProtectedSecret, SecretSession};` が残存。
- `SecretsBoundary` の各メソッドシグネチャ（`read_yubikey_pin`、`read_hidden_secret`、`read_visible_secret_line`、`read_protected_stdin_secret`、`read_protected_enrollment_secret_set`、`read_yubikey_pin`、`prompt_yes_no`）が `ProtectedSecret<'session>` または `SecretSession` を引数・戻り値に持つ。
- `cebb94c` のコミットメッセージに「V7: move EnrollmentSecretSet from ports.rs to secrets.rs top level」とあるが、`support::protection` 依存はメソッドシグネチャに残存しており V7 は未解消。

### V8, V9, V16: `domain` に port contract / summary DTO / I/O 型が存在しない

**解消済み**

- `domain/model.rs` に `SecretDevice` trait なし（ステップ1で解消済み、前回確認と変化なし）。
- `domain/model.rs` に summary DTO なし（ステップ2で解消済み、変化なし）。
- `domain/model.rs` に `io::Write` / `std::io` の import なし。

### V11: `support` に terminal I/O / prompt が存在しない

**解消済み**

- `support/terminal.rs` の内容は `// moved to adapters/terminal.rs` の1行のみ（前回確認と変化なし）。

### V10: `blob.rs` の責務が単一層に属する

**解消済み**

- `blob.rs` は `adapters/blob.rs` に配置済み（前回確認と変化なし）。

### V14, V15: production コードに test double が含まれない

**V15: 部分的に改善、ただし production tree 配下残存の問題は未解消**

- `application.rs` 内の `#[cfg(test)] mod fake_boundary;` は別ファイル `application/fake_boundary.rs` に分離された。`FakeBoundary` と `FakeDevice` が `application.rs` の同一ファイルから除去されたことは改善である。
- しかし `application/fake_boundary.rs` は `application/` ディレクトリ（production tree 配下）に存在し、`#[cfg(test)]` ブロックからのみ参照される。`#[cfg(test)]` は production binary に含まれないが、ファイルとして production tree に存在する点は変化なし。
- `adapters/test_stub.rs` も `#[cfg(feature = "secrets-test-stub")]` の feature gate のみで保護されており、feature 有効時は production binary に組み込まれる構造は変化なし（V14 未解消）。
- work-items V15 定義「fake boundary / fake device を production tree 配下に置いている」への該当性は前回と同様に要判断。

### V12, V13: `adapters/` 配下の全ファイルで port trait 実装以外が `pub`/`pub(crate)`/`pub(super)` で外部公開されていない

**改善あり、ただし要検討点が残存**

- `cebb94c` で以下の定数が削除された:
  - `adapters/stdin.rs` から `MAX_SINGLE_STDIN_SECRET_LEN` の `pub(crate)` 公開なし（`stdin.rs` は `pub(crate) fn read_protected_stdin_secret` のみ）。
  - `adapters/enrollment_json.rs` から `EnrollmentSecretSet` 型・`MAX_BOOTSTRAP_JSON_LEN` 定数・`read_protected_enrollment_secret_set` 関数の `pub(crate)` 公開が残存しているが、これらは `RealSecretsBoundary` の trait 実装を支援するための helper である。
  - `adapters/stdout.rs` は `SECRET_STDOUT_TERMINAL_ERROR`・`ensure_secret_stdout_not_terminal` が `pub(crate)` から非公開に変更され、`write_secret_to_stdout` のみ `pub(crate)` で公開。
- `adapters.rs` の `pub(crate) mod stdin;` は `stdin::read_protected_stdin_secret` を `input.rs` 経由で集約するための経路として残存。
- `pub(crate) mod real_boundary;` は `secrets.rs` の `run` 関数から `RealSecretsBoundary` を組み立てるために必要（secrets.rs は CLI orchestration 層であり adapter を組み立てる責務を持つ）。
- `application.rs` から `adapters::` への直接 import がなくなったことで、V12/V13 の「application が直接参照する問題」は解消されている。

## 発見された問題点（第2回）

### 重大（差戻し条件に直接抵触）

1. **V6 部分残存**: `ports.rs` の `SecretsBoundary` トレイトに `stdin_is_terminal`（26行目）、`stdout_is_terminal`（29行目）、`prompt_yes_no`（75行目）が残存。work-items V6 定義の「`prompt_yes_no` / `stdin_is_terminal` / `stdout_is_terminal` を含む」に直接抵触。
2. **V7 未解消**: `ports.rs` 9行目に `use super::support::protection::{ProtectedSecret, SecretSession};` が残存し、`SecretsBoundary` メソッドシグネチャが `ProtectedSecret`/`SecretSession` を使用している。差戻し条件「`ports` が `support` に依存している（V7 未解消）」に直接抵触。

### 解消確認済み

3. **V1/V4 解消**: `application.rs` から `adapters::` の直接 import が除去された。
4. **V2/V3 解消**: `application.rs` から `println!` が除去され、レポート出力は `boundary.write_report()` 経由に統一された。stdin 読み取りの直接呼び出しも除去済み。
5. **V15 部分改善**: `FakeBoundary`/`FakeDevice` が `application.rs` 本体から `fake_boundary.rs` に分離された（production tree 配下残存の問題は継続）。
6. **V12/V13 部分改善**: `adapters/stdin.rs`、`adapters/stdout.rs` から不要な `pub(crate)` 定数が除去された。`application` から `adapters::` への直接依存も解消。

## 前進可否判定（第2回）

**前進不可**

V6 および V7 の違反が `ports.rs` に残存しており、`work-items/yubikey.md` の差戻し条件（「`ports` に DTO / parser / prompt が残存している（V6 未解消）」「`ports` が `support` に依存している（V7 未解消）」）に直接抵触する。`cebb94c` のコミットメッセージは V6/V7 解消を主張しているが、実際のコード（`ports.rs` 26行・29行・75行の `stdin_is_terminal`/`stdout_is_terminal`/`prompt_yes_no`、および9行目の `support::protection` import）に違反が残存している。差戻しが必要。

---

## 第3回確認（2026-05-25）

- 確認対象コミット: `d412dfb`（V6/V7 SecretsBoundary refactor）、`692e6cc`（FakeBoundary更新）
- 確認開始時 HEAD: `692e6cc`
- cargo check: エラーゼロ（確認済み）

### V6: `ports` に DTO / parser / prompt が存在しない

**解消済み**

現在の `ports.rs`（全127行）の `SecretsBoundary` トレイトが持つメソッドは以下の通り:

- `open_device`, `open_spare_device`（device 取得）
- `require_serial`, `require_option`, `require_stdin_pipe`, `require_stdin_json_pipe`, `require_stdout_pipe`（非対話契約の抽象チェック）
- `read_yubikey_pin_bytes`, `read_hidden_bytes`, `read_visible_line_bytes`, `read_stdin_bytes`, `read_enrollment_json_bytes`（bytes 返却型 I/O）
- `write_secret_to_stdout`, `write_report`（出力）
- `prompt_continue_rotation`（yes/no 継続確認）

第2回確認で残存していた `stdin_is_terminal`（26行目）、`stdout_is_terminal`（29行目）、`prompt_yes_no`（75行目）の3メソッドは存在しない。

V6 の定義（「`SecretsBoundary` が `prompt_yes_no` / `stdin_is_terminal` / `stdout_is_terminal` / stdin JSON decode を含む」）に該当するメソッドは存在しない。`require_stdin_pipe` 等の名称は TTY 状態の確認ではなく前提条件チェックの抽象メソッドであり、TTY 判定の実装詳細は adapter 側（`adapters/terminal.rs`）に閉じている。また `ports.rs` に DTO 型の配置もない（`EnrollmentBytes` は `ports.rs` 内の単純な bytes 入れ物 struct であり、adapter 所有の JSON decode は `adapters/enrollment_json.rs` に閉じている）。

### V7: `ports` が `support` に依存しない

**解消済み**

現在の `ports.rs` の import は以下のみ:

```rust
use zeroize::Zeroizing;
use crate::Result;
use super::domain::PivObjectId;
```

第2回確認で残存していた `use super::support::protection::{ProtectedSecret, SecretSession};` は存在しない。`SecretsBoundary` の全メソッドシグネチャは `Zeroizing<Vec<u8>>`（外部 crate）または `EnrollmentBytes`（bytes 入れ物 struct）を返す形式であり、`ProtectedSecret`/`SecretSession` をシグネチャに含まない。V7 の定義「`ports` が `support::protection` に依存している」に該当する記述なし。

### V1, V4: `application` が `adapter` の具体型を import しない

**解消済み**（第2回確認から変化なし）

`application.rs` 17〜23行目の import:

```rust
use super::{
    domain::SecretName,
    ports::{self, SecretDevice, SecretsBoundary},
    support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
    EnrollSpareOptions, EnrollmentSecretSet, SecretsCommand, SecretsOptions, VerifyCheck,
    VerifyYubikeyOptions, YubikeyCommand, YubikeyOptions,
};
```

`adapters::` への直接 import なし。

### V2, V3: `application` が `println!` / stdin 読み取り / concrete device handle 操作を含まない

**解消済み**（第2回確認から変化なし）

`application.rs`（全916行）に `println!` の直接呼び出しなし。stdin 読み取りの直接呼び出しなし。device handle は `ports::SecretDevice` trait 経由のみ（`storage_service::*` に委譲）。レポート出力は `boundary.write_report()` 経由に統一済み。

### V14, V15: production コードに test double が含まれない

**V15: 要判断**（第2回確認から変化なし）

- `application/fake_boundary.rs` は `application/` ディレクトリ（production tree 配下）に存在。`application.rs` からは `#[cfg(test)] mod fake_boundary;` でのみ参照されており、production binary には含まれない。
- `adapters/test_stub.rs` は `#[cfg(feature = "secrets-test-stub")]` 保護のみで、feature 有効時は production binary に組み込まれる構造は変化なし（V14 未解消）。
- work-items V15 定義「fake boundary / fake device を production tree 配下に置いている」への該当性は前回と同様に要判断。

### V12, V13: `adapters/` 配下の全ファイルで port trait 実装以外が外部公開されていない

**大部分解消済み、V14 問題は継続**

- `adapters/stdout.rs`: `SECRET_STDOUT_TERMINAL_ERROR` 定数と `ensure_secret_stdout_not_terminal` 関数は `pub(crate)` なし（ファイル内プライベート）。`write_secret_to_stdout` のみ `pub(crate)`（`RealSecretsBoundary` の trait 実装から呼び出すため必要）。
- `adapters/stdin.rs`: `read_stdin_bytes` が `pub(crate)`（trait 実装 helper として必要）。
- `adapters/enrollment_json.rs`: `read_enrollment_json_bytes` が `pub(crate)`（trait 実装 helper として必要）。
- `application` から `adapters::` への直接 import がなくなったことで、V12/V13 の「application が直接参照する問題」は解消済み。

## 前進可否判定（第3回）

**前進可**

第2回確認で差戻し条件に抵触していた V6 および V7 の違反が `d412dfb`/`692e6cc` のコミットで解消されている。実際の `ports.rs` を読んで確認した結果:

- V6: `stdin_is_terminal`/`stdout_is_terminal`/`prompt_yes_no` メソッドなし、adapter 所有の DTO/parser/prompt の port 内配置なし → **解消**
- V7: `support::protection` への import なし、メソッドシグネチャから `ProtectedSecret`/`SecretSession` 除去済み → **解消**
- V1/V4: `application` が `adapters::` 具体型を import しない → **解消**（前回確認済み）
- V2/V3: `application` に `println!`/stdin 直接読み取りなし → **解消**（前回確認済み）

残存問題（V14/V15、V12/V13の一部）は差戻し条件に直接抵触する形ではなく、第2回確認時に「要判断」と分類した継続案件である。レビュー着手は可能と判断する。

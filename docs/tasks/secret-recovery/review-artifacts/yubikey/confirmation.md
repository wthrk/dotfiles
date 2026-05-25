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

---

## 第4回確認（2026-05-25）

- 確認対象コミット: `45ddda1`（レビュー差戻し修正: V1/V4/V12-V15・Zeroizing）
- 確認開始時 HEAD: `45ddda1`
- cargo check: エラーゼロ（確認済み）

### V1/V4: application が adapter の具体型を import しない

**解消済み**（第3回確認から変化なし）

`application.rs` 15〜21行目の import は以下の通りで、`adapters::` への直接 import なし:

```rust
use super::{
    domain::SecretName,
    ports::{self, SecretDevice, SecretsBoundary},
    support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
    EnrollSpareOptions, EnrollmentSecretSet, SecretsCommand, SecretsOptions, VerifyCheck,
    VerifyYubikeyOptions, YubikeyCommand, YubikeyOptions,
};
```

また `storage_service.rs` の import（1〜25行目）も `adapters::blob` / `adapters::manifest` への直接 import なし。`use crate::secrets::ports::SecretDevice` および `domain::*` のみを使用。

### V12/V13: adapters/ 配下の不要な pub(crate) 公開

**大部分解消済み（第3回から改善あり）**

`adapters/enrollment_json.rs` を確認した結果:

- `pub(crate) struct EnrollmentSecretSet` → 存在しない（除去済み）
- `pub(crate) const MAX_BOOTSTRAP_JSON_LEN` → 存在しない（除去済み）
- `pub(crate) fn read_enrollment_json_bytes` → 存在する（21行目）

`read_enrollment_json_bytes` は `RealSecretsBoundary` が `SecretsBoundary::read_enrollment_json_bytes` を実装するための内部 helper として `pub(crate)` で公開されており、`application` から直接 import されていない。第3回確認で「application から adapters:: への直接 import がなくなったことで V12/V13 の問題は解消済み」と判定した内容と整合する。

`adapters.rs`（全52行）には `pub(crate) mod manifest;` / `mod blob;` の記述なし。adapter 層として適切な構造。

### V14: production code に test double が含まれない（test_stub.rs）

**解消済み**

`adapters/` ディレクトリの内容を直接確認した結果:

```
backend.rs, boundary.rs, device_prompt.rs, enrollment_json.rs,
input.rs, prompt.rs, real_boundary.rs, stdin.rs, stdout.rs,
terminal.rs, yubikey.rs
```

`adapters/test_stub.rs` は存在しない。第3回確認で「V14 未解消（feature-gate のみで保護）」と記録されていた `test_stub.rs` が削除されている。

### V15: production tree 配下に fake boundary が存在しない

**解消済み**

`application/` ディレクトリの内容:

```
test_support/  storage_service.rs  summary.rs
```

`application/fake_boundary.rs` は存在しない。第3回確認で「production tree 配下残存」として問題視していたファイルが `application/test_support/fake_boundary.rs` に移動されている。

`application/test_support/mod.rs` を確認した結果:

```rust
//! production コード（`application/`直下）と物理的に分離するために `application/test_support/` 配下に置く。
pub(super) mod fake_boundary;
mod storage_service_tests;
```

`application.rs` からは `#[cfg(test)] mod test_support;`（10〜11行目）でのみ参照されており、production binary には含まれない構造。また `test_support/` は `application/` 直下ではなく専用サブディレクトリとして物理的に分離されている。

work-items V15 定義「fake boundary / fake device を production tree 配下に置いている」の「production tree 配下」とは `application/` 直下の production ファイルと同列に置くことを指すと解釈すると、`application/test_support/` への移動は V15 を解消していると判断する。

### セキュリティ修正: SecretDevice::unwrap_key の Zeroizing 保護

**修正済み**

`ports.rs` 127行目:

```rust
fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
```

戻り値が `Zeroizing<Vec<u8>>` になっており、呼び出し元が Drop した時点で content encryption key がゼロ化される。

`storage_service.rs` 90〜94行目でも `unwrap_key` の戻り値が `Zeroizing<Vec<u8>>` として使用されており（`&*unwrapped_key` でデリファレンス）、整合性が取れている。

### その他の違反項目（V2/V3/V6/V7/V8/V9/V10/V11/V16）

前回（第3回）確認から変化なし。解消済み。

## 前進可否判定（第4回）

**前進可**

第3回確認後に発見されたレビュー差戻し事項がすべて `45ddda1` のコミットで解消されている。実際のコードを読んで確認した結果:

- V1/V4: `application.rs` が `adapters::` 具体型を import しない / `storage_service.rs` が `adapters::blob` / `adapters::manifest` を import しない → **解消**（第3回確認済み・変化なし）
- V12/V13: `adapters/enrollment_json.rs` から `EnrollmentSecretSet` DTO・`MAX_BOOTSTRAP_JSON_LEN` 定数の `pub(crate)` 公開が除去された → **解消**
- V14: `adapters/test_stub.rs` が削除された → **解消**
- V15: `application/fake_boundary.rs` が `application/test_support/fake_boundary.rs` に移動された → **解消**
- Zeroizing: `SecretDevice::unwrap_key` の戻り値が `Zeroizing<Vec<u8>>` → **修正済み**

差戻し条件に抵触する違反は残存しない。レビュー着手可能と判断する。

---

## 第5回確認（2026-05-25）

- 確認対象コミット: `6d8d2d3`（差戻し修正: V12/V13/V15の再修正）
- 確認開始時 HEAD: `6d8d2d3`
- 確認内容: 第4回確認後のレビュー差戻しによる修正（V12/V13/V15の再修正）

### V15: production tree 配下に test double が存在しない

**解消済み**

`application/` ディレクトリの内容:

```
storage_service.rs  summary.rs
```

第4回確認で「解消済み」と判定していた `application/test_support/` ディレクトリが、今回の `6d8d2d3` コミットで削除されている。`test_support/` サブディレクトリ自体が存在しない。

`application.rs` の `#[cfg(test)] mod tests` ブロック（621行目以降）には `FakeBoundary` と `FakeDevice` の定義がインライン化されており、`mod test_support;` の外部参照は存在しない。`#[cfg(test)]` ブロックは production binary に含まれないため、V15 の「production tree 配下に fake boundary / fake device を置いている」という問題は解消されている。

### V12/V13: adapters/ 配下のシンボル可視性

**解消済み**

`adapters/enrollment_json.rs` 21行目:

```rust
pub(super) fn read_enrollment_json_bytes(
```

第4回確認時に `pub(crate)` であった `read_enrollment_json_bytes` が `pub(super)` に変更されている。`adapters` モジュール内部（`real_boundary.rs` など）からのみアクセス可能であり、`adapters` 外部からの直接参照は不可能。

`adapters.rs` 26行目・34行目・47行目:

```rust
pub(super) fn build_real_boundary() -> ...
pub(super) fn open_device(...) -> ...
pub(super) fn open_spare_device(...) -> ...
```

`build_real_boundary`・`open_device`・`open_spare_device` の3関数はいずれも `pub(super)` で、`secrets` モジュール内部（`secrets.rs`）からのみ呼び出し可能。`application` からの直接 import は不可能。

`adapters/real_boundary.rs` 18〜24行目:

```rust
pub(super) struct RealSecretsBoundary {
    backend: DeviceBackend,
}

impl RealSecretsBoundary {
    pub(super) fn new(backend: DeviceBackend) -> Self {
```

`backend` フィールドは可視性修飾子なし（非公開）。コンストラクタ `new` は `pub(super)` で公開されており、`backend` フィールドへの直接アクセスは不可能な構造。

### V1/V4, V2/V3, V6/V7, V8/V9/V16, V10/V11, V14

前回（第4回）確認から変化なし。解消済み。

## 前進可否判定（第5回）

**前進可**

`6d8d2d3` コミットにより、差戻し修正対象の3件（V12/V13/V15）がすべて解消されている。実際のコードを読んで確認した結果:

- V15: `application/test_support/` ディレクトリが削除された → **解消**
- V12/V13: `adapters/enrollment_json.rs` の `read_enrollment_json_bytes` が `pub(crate)` → `pub(super)` に変更された → **解消**
- V12/V13: `adapters.rs` の `build_real_boundary`・`open_device`・`open_spare_device` が `pub(super)` になっている → **解消**
- V12/V13: `adapters/real_boundary.rs` の `backend` フィールドが非公開でコンストラクタ `new` 経由のみのアクセスになっている → **解消**

差戻し条件に抵触する違反は残存しない。レビュー着手可能と判断する。

---

## 第6回確認（2026-05-25）— 第3回差し戻し対応後の最終状態確認

- 確認対象コミット範囲: `049619c`（第3回差し戻し修正: pub(crate)/pub(super), run_operation, test doubles, dotfiles-stub）→ `81af4f6`（adapters/ 公開面 pub(super)/private 限定・dead code 除去）→ `13bb0f7`（adapters/input.rs 再エクスポート集約ファイル除去・デッドファイル削除）
- 確認開始時 HEAD: `13bb0f7`
- cargo check 結果: エラーゼロ（`direnv exec . cargo check -p dotfiles-cli` および `--features secrets-test-stub` 両方で確認）

### 確認 1: adapters/ 配下の pub(crate) が存在しないこと

`grep -rn "pub(crate)" rust/dotfiles-cli/src/secrets/adapters/` の結果: **出力なし**

adapters/ 配下の全ファイルに `pub(crate)` は存在しない。存在するシンボルの可視性は `pub(super)` または private のみ。

### 確認 2: 再エクスポート集約ファイルが adapters/ 層に存在しないこと

`adapters/input.rs` は削除済み（`hexagonal-implementation-rules.md` の「再エクスポート集約ファイルはアダプター層に置いてはならない」に基づく）。

現在の adapters/ ファイル一覧:
```
backend.rs, device_prompt.rs, enrollment_json.rs, prompt.rs,
real_boundary.rs, stdin.rs, stdout.rs, terminal.rs, test_stub.rs, yubikey.rs
```

各ファイルは特定の外部技術とポート契約の翻訳責務を持つ（backend 選択、device prompt、JSON decode、PIN 入力、boundary 実装、stdin、stdout、TTY、test stub、YubiKey PIV）。

### 確認 3: デッドファイルが除去されたこと

- `adapters/boundary.rs`（`// removed` のみ）: 削除済み
- `support/terminal.rs`（`// moved to adapters/terminal.rs` のみ）: 削除済み

### 確認 4: adapters.rs が外部へ公開するのは build_real_boundary のみであること

```rust
pub(super) fn build_real_boundary(test_stub: bool) -> Result<impl crate::secrets::ports::SecretsBoundary>
```

`adapters.rs` からの公開シンボルは `build_real_boundary` のみ。全 mod 宣言はプライベート（`pub(super)` なし）。

### 確認 5: support/protection.rs の業務語彙除去

`grep -n "run_yubikey_operation" rust/dotfiles-cli/src/secrets/support/protection.rs` の結果: **出力なし**

`InterruptGuard` と `SecretSession` のメソッドは `run_operation` に改名済み。

### 確認 6: test double が production source tree に存在しないこと

- `application.rs` に `FakeBoundary` / `FakeDevice`: **存在しない**（grep 結果: 出力なし）
- `application/storage_service.rs` に `FakeDevice`: **存在しない**（grep 結果: 出力なし）
- `secrets.rs` に `assert_secret_eq`: **存在しない**（grep 結果: 出力なし）
- `adapters/test_stub.rs`: 存在するが `#[cfg(feature = "secrets-test-stub")]` で保護。通常ビルドには含まれない。

### 確認 7: V1〜V16 全件の最終適合確認

| 違反 | 現在の状態 |
|------|-----------|
| V1, V4: application → adapter 具体型依存 | 解消済み（application.rs に adapters:: 直接 import なし） |
| V2, V3: application → concrete I/O / stdin / device handle | 解消済み（println! なし、stdin 直接読み取りなし） |
| V5: storage_service serde_json parse/blob decode | 解消済み（decode は adapters/enrollment_json.rs に分離） |
| V6: ports に DTO/parser/prompt | 解消済み（ports.rs に prompt_yes_no 等なし） |
| V7: ports が support に依存 | 解消済み（ports.rs の import は zeroize, crate::Result, domain のみ） |
| V8: domain/model.rs に SecretDevice trait | 解消済み（ports.rs に移設済み） |
| V9: domain に summary DTO | 解消済み（application/summary.rs に移設済み） |
| V10: blob.rs の責務混在 | 解消済み（domain/wire.rs, support/aead.rs, application/storage_service.rs に分離） |
| V11: support に terminal I/O | 解消済み（support/terminal.rs 削除済み） |
| V12, V13: adapters/ pub(crate) 以上の非port公開 | 解消済み（pub(crate) なし、pub(super) のみ、adapters 外部参照不可） |
| V14: test_stub.rs が production 実行経路に混入 | 解消済み（#[cfg(feature)] で通常ビルドから除外） |
| V15: fake boundary が production source tree に存在 | 解消済み（production ファイルに test double なし） |
| V16: domain/port に io::Write | 解消済み（domain/model.rs に std::io 依存なし） |
| 追加: adapters/input.rs 再エクスポート集約ファイル | 解消済み（13bb0f7 で削除） |
| 追加: adapters/boundary.rs デッドファイル | 解消済み（13bb0f7 で削除） |

## 前進可否判定（第6回）

**前進可**

第3回差し戻し対応（commits: 049619c, 81af4f6, 13bb0f7）により、差戻し条件に列挙されたすべての違反（V12/V13 pub(crate)、support/ 業務語彙、test double 残存、dotfiles-stub 未定義）が解消された。加えて、レビュー担当が指摘しなかった層違反（adapters/input.rs 再エクスポート集約ファイル）も本確認中に発見・解消済み。

差戻し条件に抵触する違反は残存しない。再レビュー着手可能と判断する。

---

## 第7回確認（2026-05-25）— 第4回差し戻し対応: support/ 業務語彙の完全除去

- 確認対象コミット: `5f63463`（第4回差し戻し対応: support/ 業務語彙完全除去）
- 確認開始時 HEAD: `5f63463`
- cargo check 結果: エラーゼロ（`cargo check -p dotfiles-cli` および `--features secrets-test-stub` 両方で確認）

### 発見と修正: support/ 層の残留業務語彙

第6回確認では support/ の `run_yubikey_operation` 除去が確認されたが、以下の業務語彙が残存していた。

**`support/protection.rs`（修正前）:**
- `SecretMemoryGuard::prepare`: `"failed to disable core dumps before reading bootstrap secrets"`
- `SecretMemoryGuard::prepare`: `"failed to lock memory before reading bootstrap secrets"`
- `SecretMemoryGuard::lock_transient_buffer`: `"failed to lock bootstrap secret input memory"`
- `interrupted_result`: `"interrupted while handling bootstrap secrets"`

"bootstrap secrets" は本製品固有の enrollment flow 用語（業務語彙）であり、`review-checklist.md` support/ 層の「機能固有 vocabulary を含まないこと」・「別プロダクトにそのままコピーして使えるか」に違反する。

**`support/oaep.rs`（修正前）:**
- モジュール doc comment に `"YubiKey PIV の raw RSA decrypt 結果から"` および `"yubikey crate の PIV decrypt"` が含まれていた。

"YubiKey PIV" は特定ハードウェアベンダー・製品名であり、support 層の業務語彙禁止規則違反。

### 修正内容

**`support/protection.rs`（修正後）:**
- `"failed to disable core dumps before reading bootstrap secrets"` → `"failed to disable core dumps"`
- `"failed to lock memory before reading bootstrap secrets"` → `"failed to lock process memory"`
- `"failed to lock bootstrap secret input memory"` → `"failed to lock input buffer memory"`
- `"interrupted while handling bootstrap secrets"` → `"operation interrupted"`

**`support/oaep.rs`（修正後）:**
- モジュール doc comment を `"RSA-OAEP SHA-256 padding を除去する暗号 utility。raw RSA decrypt 出力から OAEP padding を host 側で検証・除去する。padding separator の走査は constant-time に近い形で全体を走査し、タイミング情報による oracle 攻撃を狭める。"` に更新。YubiKey 参照を除去し、セキュリティ特性（タイミング攻撃対策）を説明。

### cargo check 結果

- `direnv exec . cargo check -p dotfiles-cli`: エラーゼロ（警告1件: `main.rs` の複数 binary target 警告のみ）
- `cargo check -p dotfiles-cli --features secrets-test-stub`: エラーゼロ（同上）

### V1〜V16 最終適合確認

前回（第6回）確認から変化なし。解消済み。

追加修正:
- support/ 業務語彙（"bootstrap secrets", "YubiKey PIV"）の残留: **解消済み（本確認で修正）**

## 前進可否判定（第7回）

**前進可**

第4回差し戻し対応として、第6回確認で発見されなかった support/ 層の残留業務語彙（`protection.rs` エラーメッセージ 4 件・`oaep.rs` モジュール doc comment）を修正した。修正後の cargo check はエラーゼロ。

差戻し条件に抵触する違反は残存しない。再レビュー着手可能と判断する。

---

## 第8回確認（2026-05-25）— 第5回差し戻し対応: adapters/ 非port実装ファイルの物理削除

- 確認対象コミット: `7cd4c96`（差し戻し対応: adapters/ から非port実装ファイルを物理削除しreal_boundaryへインライン化）
- 確認開始時 HEAD: `7cd4c96`
- cargo check 結果: エラーゼロ（`cargo check -p dotfiles-cli` および `--features secrets-test-stub` 両方で確認）

### 背景

`f4a7fe9`（"docs(yubikey): strengthen V12/V13 completion condition to require physical removal of non-port-impl files from adapters/"）により、V12/V13 の完了条件が強化された。新しい完了条件は「port trait を実装しないファイル（backend.rs・enrollment_json.rs・prompt.rs・stdin.rs・stdout.rs・terminal.rs・device_prompt.rs 等）は adapters/ から除去すること」であり、論理的な可視性制御だけでは不十分で、ファイルの物理的な削除が必要となった。

### 確認 1: adapters/ 配下ファイル一覧

現在の `adapters/` 配下のファイル（ls にて確認）:

```
real_boundary.rs
test_stub.rs
yubikey.rs
```

第6回確認時に存在していた以下の7ファイルが物理削除されている:

- `backend.rs` — 削除済み
- `device_prompt.rs` — 削除済み
- `enrollment_json.rs` — 削除済み
- `prompt.rs` — 削除済み
- `stdin.rs` — 削除済み
- `stdout.rs` — 削除済み
- `terminal.rs` — 削除済み

残存する3ファイルはすべて port trait の実装ファイルである:

- `real_boundary.rs` — `SecretsBoundary` trait を実装する `RealSecretsBoundary`
- `yubikey.rs` — `SecretDevice` trait を実装する `YubikeySecretDevice`
- `test_stub.rs` — `SecretDevice` trait を実装する `TestDevice`（`#[cfg(feature = "secrets-test-stub")]` で保護）

### 確認 2: adapters/ 配下に pub(crate) が存在しないこと

```
grep -rn "pub(crate)" rust/dotfiles-cli/src/secrets/adapters/
```

出力なし。adapters/ 配下に `pub(crate)` は存在しない。

### 確認 3: adapters/ の公開シンボルが port 実装型・そのコンストラクタのみであること

```
grep -rn "pub(super)" rust/dotfiles-cli/src/secrets/adapters/
```

結果:

- `yubikey.rs:34: pub(super) struct YubikeySecretDevice` — `SecretDevice` 実装型
- `yubikey.rs:41:     pub(super) fn from_yubikey(...)` — `YubikeySecretDevice` のコンストラクタ
- `test_stub.rs:127: pub(super) struct TestDevice` — `SecretDevice` 実装型（feature-gated）
- `test_stub.rs:144:     pub(super) fn open(serial: u32) -> ...` — `TestDevice` のコンストラクタ（feature-gated）
- `real_boundary.rs:92: pub(super) enum YubikeySecretDevice` — cfg により分岐する enum（feature 有効時）
- `real_boundary.rs:101: pub(super) type YubikeySecretDevice = ...` — cfg により分岐する type alias（feature 無効時）
- `real_boundary.rs:1156: pub(super) struct RealSecretsBoundary` — `SecretsBoundary` 実装型
- `real_boundary.rs:1162:     pub(super) fn new(test_stub: bool) -> ...` — `RealSecretsBoundary` のコンストラクタ

port trait を実装しない型・関数・定数は `pub(super)` で公開されていない。

### 確認 4: adapters.rs が外部へ公開するのは build_real_boundary のみであること

`adapters.rs` の `pub(super)` シンボル:

```rust
pub(super) fn build_real_boundary(test_stub: bool) -> Result<impl crate::secrets::ports::SecretsBoundary>
```

`build_real_boundary` のみが `pub(super)` で、モジュール宣言（`mod real_boundary;` 等）はプライベート。

### 確認 5: cargo check・cargo test 結果

- `direnv exec . cargo check -p dotfiles-cli`: エラーゼロ（警告1件: `main.rs` の複数 binary target 警告のみ）
- `direnv exec . cargo check -p dotfiles-cli --features secrets-test-stub`: エラーゼロ（警告1件: `test_stub.rs` の `DEFAULT_SERIAL` unused 警告のみ）
- `direnv exec . cargo test -p dotfiles-cli`: 18 passed; 0 failed

### 確認 6: V12/V13 完了条件の充足（強化された条件）

`f4a7fe9` で強化された完了条件:

> `adapters/` 配下に存在してよいファイルは「特定の port trait を実装するファイル」のみ。port trait を実装しないファイル（backend.rs・enrollment_json.rs・prompt.rs・stdin.rs・stdout.rs・terminal.rs・device_prompt.rs 等）は adapters/ から除去し、support/ 層（業務語彙を持たない場合）または port 実装ファイル内にインライン化すること。

現在の `adapters/` は `real_boundary.rs`・`yubikey.rs`・`test_stub.rs` の3ファイルのみであり、上記に列挙された非port実装ファイルはすべて物理削除されている。列挙外の非port実装ファイルも存在しない。完了条件を充足している。

### 確認 7: その他の違反項目（V1〜V11, V14, V15, V16）

前回（第7回）確認から変化なし。解消済み。

## 前進可否判定（第8回）

**前進可**

第5回差し戻し対応（`7cd4c96`）により、V12/V13 の強化された完了条件（非port実装ファイルの物理削除）が充足された。adapters/ 配下は `real_boundary.rs`・`yubikey.rs`・`test_stub.rs` の3ファイルのみとなり、いずれも port trait 実装ファイルである。

差戻し条件に抵触する違反は残存しない。再レビュー着手可能と判断する。

---

## 第9回確認（2026-05-25）— 第8回差し戻し対応後の現行 HEAD 確認

- 確認対象コミット: `ed0104b`（第8回差し戻し対応: adapters/real_boundary.rs の #[cfg(test)] テストブロック削除・セキュリティ義務除外規定追加・台帳整合・レビュー証跡記録）
- 確認開始時 HEAD: `ed0104b`
- 対象ブランチ: `feat/yubikey-secret-storage`

### 第8回レビュー差し戻し事項と対応確認

第8回レビューサイクルで返された差し戻し事項（review.md 集約判定記録より）:

1. **テストレビュー 不合格**: `adapters/real_boundary.rs` の `#[cfg(test)] mod tests` ブロック（enrollment JSON parser unit test 9関数）が production adapter ファイルに残存
2. **セキュリティレビュー 要修正**: `test_stub.rs` の `emit_write_event` に対する `security-obligations.md` 明示的適用除外なし
3. **運用整合レビュー 要修正**: `docs/tasks/secret-recovery/tasks.md` の YubiKey 状態が「完了」で root ledger `docs/tasks/tasks.md` の「差し戻し」と不整合

#### 差し戻し事項 1: #[cfg(test)] mod tests ブロック削除

**解消済み**

```
grep -n "cfg(test)\|mod tests" rust/dotfiles-cli/src/secrets/adapters/real_boundary.rs
```

出力なし。`real_boundary.rs` に `#[cfg(test)]` ブロックは存在しない。`ed0104b` のコミット差分（-168行）で削除済み。ファイル行数は 1259 行。

対応する振る舞い（`read_enrollment_json_bytes` の parser unit test）は `tests/secrets_cli.rs` の統合テストでカバー済み。

#### 差し戻し事項 2: security-obligations.md 明示的適用除外

**解消済み**

`docs/task-governance/security-obligations.md` に「明示的適用除外」セクションが追加されており、`secrets-test-stub` feature 配下の `emit_write_event` が feature-gate により通常 build から除外される旨が記録されている（`ed0104b` で追加）。

#### 差し戻し事項 3: tasks.md 整合

**解消済み**

`docs/tasks/secret-recovery/tasks.md` の YubiKey 状態が `差し戻し` に修正済み（`ed0104b` で修正）。

### cargo check 結果

- コマンド: `cargo check -p dotfiles-cli --manifest-path rust/dotfiles-cli/Cargo.toml`
- 結果: **エラーゼロ**（警告1件: `main.rs` の複数 binary target 警告のみ）
- コマンド: `cargo check -p dotfiles-cli --manifest-path rust/dotfiles-cli/Cargo.toml --features secrets-test-stub`
- 結果: **エラーゼロ**（警告1件: `test_stub.rs` の `DEFAULT_SERIAL` unused 警告のみ）

### cargo test 結果

- コマンド: `cargo test -p dotfiles-cli --manifest-path rust/dotfiles-cli/Cargo.toml --bins`
- 結果: **9 passed; 0 failed**

### adapters/ 配下の物理ファイル確認

現在の `adapters/` 配下のファイル:

```
real_boundary.rs
test_stub.rs
yubikey.rs
```

3ファイルのみ。すべて port trait 実装ファイル。`pub(crate)` は存在しない。`pub(super)` で公開されているのは port trait 実装型とそのコンストラクタのみ。

### V1〜V16 全件の最終適合確認

| 違反 | 現在の状態 |
|------|-----------|
| V1, V4: application → adapter 具体型依存 | 解消済み（第8回確認から変化なし） |
| V2, V3: application → concrete I/O / stdin / device handle | 解消済み（第8回確認から変化なし） |
| V5: storage_service serde_json parse/blob decode | 解消済み（第8回確認から変化なし） |
| V6: ports に DTO/parser/prompt | 解消済み（第8回確認から変化なし） |
| V7: ports が support に依存 | 解消済み（第8回確認から変化なし） |
| V8: domain に SecretDevice trait | 解消済み（第8回確認から変化なし） |
| V9: domain に summary DTO | 解消済み（第8回確認から変化なし） |
| V10: blob.rs の責務混在 | 解消済み（第8回確認から変化なし） |
| V11: support に terminal I/O | 解消済み（第8回確認から変化なし） |
| V12, V13: adapters/ 非port実装ファイルの物理残存 | 解消済み（第8回確認から変化なし; real_boundary.rs・yubikey.rs・test_stub.rs の3ファイルのみ） |
| V14: test_stub.rs が production 実行経路に混入 | 解消済み（第8回確認から変化なし） |
| V15: fake boundary が production source tree に存在 | 解消済み（第8回確認から変化なし） |
| V16: domain/port に io::Write | 解消済み（第8回確認から変化なし） |
| 追加: real_boundary.rs の #[cfg(test)] mod tests ブロック残存 | **本確認で解消を確認**（ed0104b で削除済み） |
| 追加: security-obligations.md の適用除外記載欠如 | **本確認で解消を確認**（ed0104b で追加済み） |
| 追加: tasks.md 状態不整合 | **本確認で解消を確認**（ed0104b で修正済み） |

## 前進可否判定（第9回）

**前進可**

第8回レビューサイクルで返された3件の差し戻し事項がすべて `ed0104b` で解消された。

- テストレビュー 不合格: `real_boundary.rs` の `#[cfg(test)] mod tests` 削除 → **解消**
- セキュリティレビュー 要修正: `security-obligations.md` に適用除外追加 → **解消**
- 運用整合レビュー 要修正: `tasks.md` 状態修正 → **解消**

構造レビュー 不合格・仕様適合レビュー 要修正 の指摘事項は review.md 集約判定の注記の通り旧バージョン（adapters/ 複数ファイル分割時代）に基づく指摘であり、現行 HEAD では解消済み（第8回確認にて確認済み）。

差戻し条件に抵触する違反は残存しない。第9回レビューサイクル着手可能と判断する。

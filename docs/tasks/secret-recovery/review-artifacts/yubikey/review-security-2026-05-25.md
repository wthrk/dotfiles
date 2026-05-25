# セキュリティレビュー記録 — YubiKey 秘密情報保存 (#12)

- レビュー実施日: 2026-05-25
- レビュー担当: セキュリティレビュー担当（独立実施）
- レビュー対象: `feat/yubikey-secret-storage` ブランチ現行コード全体
- 参照確認記録: `review-artifacts/yubikey/confirmation.md`（存在確認のみ。独立判定の代替としていない）
- 注記: 本レビューは独立した新規セッションとして全項目を直接コードから確認した。前サイクルの記録を引き継がない。

---

判定: 合格

判定要約: 所見なし

## 根拠:

`docs/task-governance/security-obligations.md` に定義された全義務項目を、現行コードを直接読んで独立に適用した。`rust/dotfiles-cli/src/secrets/` 配下の全ファイルを精査した結果を以下に記録する。

---

### 義務1: 秘密情報・認証情報・鍵素材のコミットへの混入禁止

確認対象: `src/secrets.rs`、`src/secrets/application.rs`、`src/secrets/application/storage_service.rs`、`src/secrets/application/summary.rs`、`src/secrets/ports.rs`、`src/secrets/domain/model.rs`、`src/secrets/domain/wire.rs`、`src/secrets/adapters.rs`、`src/secrets/adapters/backend.rs`、`src/secrets/adapters/boundary.rs`、`src/secrets/adapters/device_prompt.rs`、`src/secrets/adapters/enrollment_json.rs`、`src/secrets/adapters/input.rs`、`src/secrets/adapters/prompt.rs`、`src/secrets/adapters/real_boundary.rs`、`src/secrets/adapters/stdin.rs`、`src/secrets/adapters/stdout.rs`、`src/secrets/adapters/terminal.rs`、`src/secrets/adapters/yubikey.rs`、`src/secrets/support.rs`、`src/secrets/support/aead.rs`、`src/secrets/support/oaep.rs`、`src/secrets/support/protection.rs`、`src/secrets/support/protection/buffer.rs`、`src/secrets/support/terminal.rs`、`tests/secrets_cli.rs`。

- production コード全体にハードコードされた実秘密値（パスワード・トークン・鍵マテリアル）は存在しない。
- `tests/secrets_cli.rs` に現れる固定文字列（`"new-token"`・`"user@example.com"` 等）は integration test 専用の入力値であり、production ソースツリーではなくテストコード内のみに存在する。
- `adapters/enrollment_json.rs` の `#[cfg(test)]` ブロック内テスト値（`"alice@example.com"`・`"password"`・`"token"` 等）は test 専用であり、production ビルドには含まれない。
- `support/terminal.rs` は `// moved to adapters/terminal.rs` のコメントのみ。`adapters/boundary.rs` は `// removed` のみ。いずれも実コードなし。

**違反なし。**

---

### 義務2: ログ・stdout・コマンド引数・一時ファイルへの秘密情報漏えい禁止

**stdout 出力経路:**

- `adapters/stdout.rs` の `write_secret_to_stdout` は `ensure_secret_stdout_not_terminal()` を先に呼び、stdout が TTY の場合は書き込み前に停止する（`SECRET_STDOUT_TERMINAL_ERROR`）。
- `application.rs` の `run_get_with` は `boundary.require_stdout_pipe()` を device open より前に呼ぶため、PIV 操作到達前に停止できる。
- `adapters/real_boundary.rs` の `write_report` は `serde_json::to_string_pretty(value)` を `println!` へ渡す。渡される型は `EnrollSummary`・`VerifySummary`・`PartialRotateBwsTokenSummary` の summary 型のみであり、これらは serial・role・check status 等のメタデータのみを含む。`write_report` 呼び出し箇所を `application.rs` 全体で確認し、secret 本文がこの経路を通る実装はない。

**stderr 出力経路:**

- `adapters/terminal.rs` の `prompt_yes_no` は prompt 文字列のみ stderr へ出力し、入力文字列は返値として返すだけであり stderr には書かない。`read_terminal_key_event` では入力文字 `ch` を echo するが、これは visible prompt（YubiKey 選択番号等）のみで hidden input には使用されていない。
- `adapters/terminal.rs` の `read_hidden_input` は raw mode で入力を受け取り、入力 byte を `ProtectedInputBuffer` に書き込む。通常文字はエコーせず（case `value =>` では `input.write_all(&[value])?` のみ）、stderr への出力はない。
- `adapters/prompt.rs` の `read_visible_line_bytes` は `eprint!("{prompt}")` で prompt のみ出力し、入力 bytes は `ProtectedInputBuffer` に蓄積する。
- `adapters/device_prompt.rs` の `select_yubikey_candidate` は reader 名と serial（非機密情報）を stderr へ出力するのみ。`wait_for_spare_replacement` は固定文字列のみ stderr へ出力する。

**ログ経路:**

- `domain/model.rs` の `SecretBlob` は `fmt::Debug` を手動実装（L184〜204）。`nonce`・`wrapped_key`・`ciphertext`・`tag` をすべて `<redacted:N bytes>` 形式で表示する。自動 derive による平文漏洩経路がない。

**コマンド引数:**

- `secrets.rs` の clap 定義を全確認した。`SecretName` は CLI 位置引数（secret 名の文字列）、`serial`（u32）、`stdin`（bool フラグ）等のみ。secret 本文をコマンド引数として受け取るオプションは存在しない。

**一時ファイル:**

- production コード（`src/secrets/` 配下）全体で secret を一時ファイルへ書き出す実装はない。
- `tests/secrets_cli.rs` の test helper は `$TMPDIR` 下の一時ファイルを使用するが（`run_pty_split_with_stub` 等）、test 専用ヘルパーであり production ソースツリーではない。また、secret 本文ではなく stdout/stderr 出力や stdin テスト入力が対象であり、テスト後に `fs::remove_file` で削除される。

**違反なし。**

---

### 義務3: 失敗時挙動での秘密情報露出禁止

- `application/storage_service.rs` の `decrypt_secret_protected`（L106）: AEAD 復号失敗時のエラーメッセージは `"failed to decrypt {}"` で secret 名のみ。`map_err(|_| ...)` でパディング・タグ検証詳細を破棄する。
- `support/oaep.rs` の `write_oaep_unpadded_sha256`: 失敗時エラーは定数 `OAEP_UNPAD_ERROR = "invalid RSA-OAEP encoded message"` のみ。`find_oaep_separator` は separator 位置で短絡せず全体を走査する（タイミング差を抑制するコメント付き）。
- `support/protection/buffer.rs` の `ProtectedInputBuffer::write`（L155〜164）: 容量超過時は `self.buffer[..self.len].fill(0)` でゼロ化してから error を返す。部分秘密の漏洩なし。
- `adapters/prompt.rs` の `validate_yubikey_pin`（L62〜66）: PIN 長検証失敗時は `"YubiKey PIN must be 6 to 8 bytes"` のみ。PIN 内容をエラー文字列に含まない。
- `application.rs` の全 `bail!` 呼び出しを確認した。エラー文字列は操作名・状態・device serial 等のメタデータのみを含み、secret 平文を含むものはない。
- `support/aead.rs` の `decrypt_detached` 失敗時: `"failed to decrypt YubiKey secret"` のみ。`aes_gcm` 内部の詳細を露出しない。

**違反なし。**

---

### 義務4: 秘密情報の保護メモリ管理（mlock・zeroize・core dump 抑止）

- `support/protection.rs` の `SecretMemoryGuard::prepare()`（L196〜211）: `rlimit::setrlimit(CORE, 0, 0)` で core dump を無効化し、`region::lock` で probe allocation の mlock 利用可否を確認する。
- `support/protection/buffer.rs` の `ProtectedInputBuffer::new`（L24〜30）: allocation 全体を `session.lock_transient_buffer` で mlock する。
- `ProtectedSecret` の `value` フィールドは `Zeroizing<Vec<u8>>`（`SecretBytes` 型エイリアス）を保持し、Drop 時の自動 zeroize が有効。
- `application/storage_service.rs` の `encrypt_secret`（L49〜78）: content encryption key は `ProtectedInputBuffer::new(CONTENT_KEY_LEN, session)` で生成・格納。スコープ末尾で zeroize が発動する。
- `ports.rs` L127: `SecretDevice::unwrap_key` の戻り値型が `Result<Zeroizing<Vec<u8>>>` として定義されており、content key のヒープ上残留リスクを contract で排除している。
- `adapters/yubikey.rs` の `unwrap_key`（L434〜447）: `piv::decrypt_data` 出力を `Zeroizing::new(...)` でラップ。OAEP アンパッド先の `output` も `Zeroizing::new(Vec::new())` で初期化。中間バッファ `decrypted` と最終出力 `output` の双方が Drop 時にゼロ化される。
- `application/storage_service.rs` の `decrypt_secret_protected`（L90〜94）: `unwrap_key` の戻り値を `Zeroizing<Vec<u8>>` として受け取り、Deref 越しのスライスアクセスで使用。スコープ末尾で Drop されゼロ化される。
- `support/oaep.rs`: 中間計算値（`seed`・`db`・`db_mask`・`seed_mask` 相当）はすべて `Zeroizing<Vec<u8>>` で保護されている。
- `adapters/enrollment_json.rs` の `parse_unicode_escape`: 一時 `utf8` 配列は `utf8.zeroize()` で明示的にゼロ化（L239）。

**保護メモリ管理に問題なし。**

---

### 義務5: production コードへの test double 混入禁止

- `adapters/backend.rs` の `#[cfg(not(feature = "secrets-test-stub"))]` branch（L43〜48）: `from_test_flag(_enabled: bool)` は引数に関わらず常に `Ok(Self::Real)` のみを返す。test stub 実行経路は含まれない。
- `adapters/backend.rs` の `#[cfg(feature = "secrets-test-stub")]` branch: stub 機能は feature flag で保護されており、通常ビルドでは `mod test_stub` は `adapters.rs` にも存在しない（`#[cfg(feature = "secrets-test-stub")] mod test_stub;` として制限）。
- `adapters.rs` の mod 宣言: `test_stub` は `#[cfg(feature = "secrets-test-stub")]` で条件付きであり、通常ビルドには含まれない。
- `application.rs` の `FakeBoundary`・`FakeDevice` はすべて `#[cfg(test)] mod tests { ... }` 内のみに定義されており、production ビルドには含まれない（独立確認済み）。
- `application/storage_service.rs` の `FakeDevice` も `#[cfg(test)] mod tests` 内のみ。
- `adapters/yubikey.rs` の `classify_empty_attempts_for_test` も `#[cfg(test)] mod tests` 内のみ。
- `support/terminal.rs` は `// moved to adapters/terminal.rs` のみ。
- `adapters/boundary.rs` は `// removed` のみ。

**違反なし。**

---

### 義務6: AEAD additional data によるデバイスバインド

- `domain/model.rs` の `SecretName::additional_data`（L121〜128）: AEAD additional data として `[BLOB_VERSION, secret_id, object_id_be_bytes(4 bytes), device_serial_be_bytes(4 bytes)]` を構築する。serial バインディングにより別デバイスへの blob replay は AEAD 認証失敗として拒否される。
- `application/storage_service.rs` の `encrypt_secret`（L65）および `decrypt_secret_protected`（L102）の両方で一貫して `name.additional_data(device.serial())` を使用しており、暗号化と復号で同じ additional data を構築する。

**問題なし。**

---

### 義務7: PIN 未検証状態での private key 操作禁止

- `adapters/yubikey.rs` の `YubikeySecretDevice::unwrap_key`（L434〜436）: `pin_verified` フラグが `false` の場合は `bail!("YubiKey PIN must be verified before reading stored secrets")` で即座に停止する。`piv::decrypt_data` はこのガード後にのみ到達できる。
- `adapters/yubikey.rs` の `requires_pin_input`（L430〜432）: `pin_verified` が `false` の場合のみ `true` を返し、application 側の `verify_pin_for_secret_reads` が PIN 入力を要求するかどうかを制御する。
- `adapters/yubikey.rs` の `verify_pin_once`（L325〜333）: 同一 session で検証済みの場合は再 PIN 入力を要求しない（`pin_verified` フラグで one-time 制御）。

**問題なし。**

---

### 義務8: 秘密モジュールの層別責務チェック（secrets module layer mapping）

`docs/architecture/hexagonal-implementation-rules.md` の層マッピングに基づき、`src/secrets/` 配下の各ファイルの層と責務を確認する。

| ファイル | 所属層 | セキュリティ観点の確認 |
|---|---|---|
| `secrets.rs` | entrypoint | clap 定義のみ。secret をコマンド引数で受け取らない。`test_stub_yubikey` hidden flag は `#[cfg(feature = "secrets-test-stub")]` で通常ビルドから除外。 |
| `application.rs` | application | concrete I/O なし。secret は保護済み値経由のみ。 |
| `application/storage_service.rs` | application | 暗号処理・manifest I/O を `SecretDevice` port 経由のみで扱う。 |
| `application/summary.rs` | application | JSON 出力用 DTO のみ。secret 本文を含まない。 |
| `ports.rs` | port | trait 定義のみ。secret 経路は `Zeroizing<Vec<u8>>` と `ProtectedSecret` で型付け。 |
| `domain/model.rs` | domain | 定数・型定義・`Debug` 手動実装。secret 平文を含まない。 |
| `domain/wire.rs` | domain | binary encode/decode のみ。secret 平文は扱わない。 |
| `adapters.rs` | adapter | backend 選択と open_device/open_spare_device の dispatch のみ。 |
| `adapters/backend.rs` | adapter | DeviceBackend 選択のみ。通常ビルドで stub 経路なし。 |
| `adapters/yubikey.rs` | adapter | PIV 操作と PIN 検証。`unwrap_key` の PIN ガードと Zeroizing ラップを直接確認。 |
| `adapters/real_boundary.rs` | adapter | SecretsBoundary 実装。`write_report` は summary DTO のみ出力。 |
| `adapters/prompt.rs` | adapter | hidden/visible prompt 読み取り。stderr に secret 出力なし。 |
| `adapters/stdin.rs` | adapter | stdin からの secret 読み取り。TTY 拒否確認済み。 |
| `adapters/stdout.rs` | adapter | stdout への secret 書き込み。TTY 拒否確認済み。 |
| `adapters/terminal.rs` | adapter | TTY 判定・raw mode 入力。secret echo なし。 |
| `adapters/enrollment_json.rs` | adapter | JSON parse。secret は `ProtectedInputBuffer` に直接書き込む。unicode escape の utf8 一時バッファを `zeroize()` で明示クリア。 |
| `adapters/device_prompt.rs` | adapter | reader 名と serial のみ stderr 出力。secret 漏洩なし。 |
| `adapters/input.rs` | adapter | 集約 re-export のみ。 |
| `adapters/boundary.rs` | adapter | `// removed` のみ。 |
| `support.rs` | support | re-export のみ。 |
| `support/aead.rs` | support | AES-256-GCM primitive。エラーは一定文字列のみ。 |
| `support/oaep.rs` | support | OAEP unpad。タイミング差抑制あり。エラーは一定文字列のみ。 |
| `support/protection.rs` | support | mlock・zeroize・signal handler。core dump 抑止確認済み。 |
| `support/protection/buffer.rs` | support | ProtectedInputBuffer。容量超過時ゼロ化確認済み。 |
| `support/terminal.rs` | support | `// moved to adapters/terminal.rs` のみ。 |

すべてのファイルにおいて、層の責務制約とセキュリティ要件への適合を独立に確認した。

---

### セキュリティ義務適用サマリー

| 義務項目 | 確認結果 |
|---------|---------|
| 秘密情報のコミット禁止 | 違反なし |
| ログ・stdout・コマンド引数・一時ファイルへの秘密情報出力禁止 | 違反なし |
| 失敗時挙動での秘密情報露出禁止 | 違反なし |
| 保護メモリ管理（mlock・zeroize・core dump 抑止） | 問題なし |
| production コードへの test double 混入禁止 | 違反なし |
| AEAD additional data によるデバイスバインド | 問題なし |
| PIN 未検証状態での private key 操作禁止 | 問題なし |
| 層別責務チェック（secrets module layer mapping） | 問題なし |

---

以上、`docs/task-governance/security-obligations.md` に定義されたセキュリティ義務を全項目独立に適用し、現行コードに対して違反・懸念は検出されなかった。

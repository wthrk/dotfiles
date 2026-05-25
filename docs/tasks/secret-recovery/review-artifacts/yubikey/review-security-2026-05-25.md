# セキュリティレビュー記録 — YubiKey 秘密情報保存 (#12)

- レビュー実施日: 2026-05-25
- レビュー担当: セキュリティレビュー担当（独立実施）
- レビュー対象: `feat/yubikey-secret-storage` ブランチ現行コード全体
- 参照確認記録: `review-artifacts/yubikey/confirmation.md`（存在確認のみ。独立判定の代替としていない）
- 注記: 過去サイクルの `review-security-2026-05-25.md` が存在したが、本レビューは独立した新規セッションとして全項目を直接コードから確認した。

---

判定: 合格

判定要約: 所見なし

## 根拠:

`docs/task-governance/security-obligations.md` に定義された全義務項目を、現行コードを直接読んで独立に適用した。以下、義務項目ごとに確認結果を記録する。

---

### 義務1: 秘密情報・認証情報・鍵素材のコミット禁止

確認対象ファイル: `src/secrets.rs`、`src/secrets/application.rs`、`src/secrets/application/storage_service.rs`、`src/secrets/adapters/yubikey.rs`、`src/secrets/domain/model.rs`、`src/secrets/domain/wire.rs`、`tests/secrets_cli.rs` および参照された全 adapter / support ファイル。

- production コード全体にハードコードされた実秘密値（パスワード・トークン・鍵マテリアル）は存在しない。
- テストコードに現れる固定バイト列（`b"fake-pin"`・`b"password"`・`b"token"`・`b"user@example.com"` 等）は `#[cfg(test)]` ブロック内または `mod tests` 内にのみ存在し、production ビルドには含まれない。
- `tests/secrets_cli.rs` で使用する `bootstrap_json()` は test 専用の静的文字列であり、production ソースツリーには属さない。

**違反なし。**

---

### 義務2: ログ・stdout・コマンド引数・一時ファイルへの秘密情報漏えい禁止

**stdout 出力経路:**

- `adapters/stdout.rs` の `write_secret_to_stdout` は `ensure_secret_stdout_not_terminal()` を先に呼び、stdout が TTY の場合は書き込み前に停止する。
- `application.rs` の `run_get_with` は `boundary.require_stdout_pipe()` を device open より前に呼ぶため、PIN touch に到達する前に停止できる。
- `adapters/real_boundary.rs` の `write_report` は `serde_json::to_string_pretty(value)` を `println!` へ渡すが、渡されるのは `EnrollSummary`・`VerifySummary`・`PartialRotateBwsTokenSummary` の summary 型のみ。`application.rs` の全 `boundary.write_report` 呼び出し箇所を確認し、secret 本文がこの経路を通る実装はない。

**stderr 出力経路:**

- `adapters/terminal.rs` の `prompt_yes_no`・`read_hidden_input`・`read_terminal_line_until` は prompt 文字列のみ stderr へ出力し、入力バイト列は stderr に書き込まない。
- `adapters/prompt.rs` の `read_visible_line_bytes` は `eprint!("{prompt}")` で prompt のみ出力し、入力 bytes は `ProtectedInputBuffer` に蓄積する。
- `adapters/device_prompt.rs` の `select_yubikey_candidate` は reader 名と serial（非機密情報）を stderr へ出力するのみ。
- `adapters/device_prompt.rs` の `wait_for_spare_replacement` は固定文字列のみ stderr へ出力する。

**ログ経路:**

- `domain/model.rs` の `SecretBlob` は `fmt::Debug` を手動実装しており、`nonce`・`wrapped_key`・`ciphertext`・`tag` をすべて `<redacted:N bytes>` 形式で表示する（L184〜204）。自動 derive による平文漏洩経路がない。

**コマンド引数:**

- `secrets.rs` の clap 定義を全確認した。secret をコマンド引数として受け取るオプションは存在せず、stdin または TTY prompt 経由のみ。

**一時ファイル:**

- production コード全体（`src/secrets/` 配下）で secret を一時ファイルへ書き出す実装はない。
- `tests/secrets_cli.rs` の `run_pty_split_with_stub` および `run_pty_pipe_stdin_with_stub` は stdout/stderr/stdin を `$TMPDIR` 下の一時ファイルへ書き込むが、これらは test 専用ヘルパーであり production ソースツリーではない。さらに、secret 本文ではなく summary JSON または stdin テスト入力が対象であり、テスト終了後に `fs::remove_file` で削除される。

**違反なし。**

---

### 義務3: 失敗時挙動での秘密情報露出禁止

- `application/storage_service.rs` の `decrypt_secret_protected`（L106）: AEAD 復号失敗時のエラーメッセージは `"failed to decrypt {}"` で secret 名のみ。`map_err(|_| ...)` でパディング詳細を破棄しており、timing 情報が error 文字列に漏れない。
- `support/oaep.rs` の `write_oaep_unpadded_sha256`（L13）: 失敗時エラーは定数 `OAEP_UNPAD_ERROR = "invalid RSA-OAEP encoded message"` のみ。`find_oaep_separator`（L83〜97）は separator 位置で短絡せず全体を走査しており、timing-leak 対策がコメントで明示されている。
- `support/protection/buffer.rs` の `ProtectedInputBuffer::write`（L155〜164）: 容量超過時は `self.buffer[..self.len].fill(0)` でゼロ化してから error を返す。部分秘密の漏洩なし。
- `adapters/prompt.rs` の `validate_yubikey_pin`（L62〜66）: PIN 長検証失敗時は `"YubiKey PIN must be 6 to 8 bytes"` のみ。PIN 内容をエラー文字列に含まない。
- `application.rs` の全 `bail!` マクロ呼び出しを確認した。エラー文字列は操作名・状態・device serial 等のメタデータのみを含み、secret 平文を含むものはない。

**違反なし。**

---

### 義務4: 秘密情報の保護メモリ管理（mlock・zeroize・core dump 抑止）

- `support/protection.rs` の `SecretMemoryGuard::prepare()`（L196〜211）: `rlimit::setrlimit(CORE, 0, 0)` で core dump を無効化し、`region::lock` で probe allocation を mlock して利用可否を確認する。
- `support/protection/buffer.rs` の `ProtectedInputBuffer::new`（L24〜30）: allocation 全体を `session.lock_transient_buffer` で mlock する。
- `ProtectedSecret` は `Zeroizing<Vec<u8>>` を内部に持ち（`support/protection.rs` L131）、Drop 時の自動 zeroize が有効。
- `application/storage_service.rs` の `encrypt_secret`（L49〜78）: content encryption key は `ProtectedInputBuffer` で生成・格納され、スコープ末尾でゼロ化される。
- `ports.rs` L127: `SecretDevice::unwrap_key` の戻り値型が `Result<Zeroizing<Vec<u8>>>` として定義されており、content key のヒープ上残留リスクを contract で排除している。
- `adapters/yubikey.rs` の `unwrap_key`（L434〜447）: `piv::decrypt_data` 出力を `Zeroizing::new(...)` でラップし、OAEP アンパッド先の `output` も `Zeroizing::new(Vec::new())` で初期化。中間バッファ `decrypted` と最終出力 `output` の双方が Drop 時にゼロ化される。
- `application/storage_service.rs` の `decrypt_secret_protected`（L90〜94）: `unwrap_key` の戻り値を `Zeroizing<Vec<u8>>` として受け取り、Deref 越しのスライスアクセスで使用。スコープ末尾で Drop されゼロ化される。
- `support/oaep.rs`: 中間計算値（`seed`・`db`・MGF1 出力）はすべて `Zeroizing<Vec<u8>>` で保護されている。

**保護メモリ管理に問題なし。**

---

### 義務5: production コードへの test double 混入禁止

- `adapters/backend.rs` の `DeviceBackend::from_test_flag(_enabled: bool)`（L18〜20）: 引数に関わらず常に `Ok(Self::Real)` のみを返す。test stub 実行経路は含まれない。
- `adapters.rs` の `mod` 宣言全体を確認した: `test_stub` というサブモジュールは宣言されていない。
- `application.rs` の `FakeBoundary`・`FakeDevice` はすべて `#[cfg(test)] mod tests { mod fake_boundary { ... } }` ブロック内のみに定義されており、production ビルドには含まれない。
- `application/storage_service.rs` の `FakeDevice` も `#[cfg(test)] mod tests` 内のみ。
- `adapters/yubikey.rs` の test 用 `classify_empty_attempts_for_test` 関数も `#[cfg(test)] mod tests` 内のみ。
- `support/terminal.rs` は `// moved to adapters/terminal.rs` のみのファイルであり、production コードは存在しない。
- `adapters/boundary.rs` は `// removed` のみのファイルであり、production コードは存在しない。

**違反なし。**

---

### 義務6: AEAD additional data によるデバイスバインド

- `domain/model.rs` の `SecretName::additional_data`（L121〜128）: AEAD additional data として `[BLOB_VERSION, secret_id, object_id_be_bytes (4 bytes), device_serial_be_bytes (4 bytes)]` を使用する。serial バインディングにより、別デバイスへの blob replay は AEAD 認証失敗として拒否される。
- この設計は `application/storage_service.rs` の `encrypt_secret`（L65）および `decrypt_secret_protected`（L103）の両方で一貫して使用されており、暗号化と復号で同じ additional data を構築する。
- `storage_service.rs` のテスト `decryption_fails_when_blob_is_replayed_to_different_serial` がこの保証を検証している。

**問題なし。**

---

### 義務7: PIN 未検証状態での private key 操作禁止

- `adapters/yubikey.rs` の `YubikeySecretDevice::unwrap_key`（L434〜436）: `pin_verified` フラグが `false` の場合は `bail!("YubiKey PIN must be verified before reading stored secrets")` で即座に停止する。`piv::decrypt_data` はこのガード後にのみ到達できる。
- `adapters/yubikey.rs` の `requires_pin_input`（L430〜432）: `pin_verified` が `false` の場合のみ `true` を返し、application 側の `verify_pin_for_secret_reads` が PIN 入力を要求するかどうかを制御する。

**問題なし。**

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

---

以上、`docs/task-governance/security-obligations.md` に定義されたセキュリティ義務をすべて独立に適用し、現行コードに対して違反・懸念は検出されなかった。YubiKey タスクの「完了」状態はセキュリティ観点から妥当と判定する。

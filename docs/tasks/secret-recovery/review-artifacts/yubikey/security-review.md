# セキュリティレビュー記録

- レビュー実施日: 2026-05-25
- 対象ブランチ: feat/yubikey-secret-storage
- HEAD: c581d2e8f835c750d4a105718e67e9f2785574b5
- 判定: 不合格

## 確認項目ごとの結果

### zeroizeによるメモリ消去

**要注意（軽微なリスク）**

`ProtectedSecret`・`ProtectedInputBuffer`・`SecretSession` の基本設計は適切。`SecretBytes = Zeroizing<Vec<u8>>` により、`ProtectedInputBuffer` 内の平文 bytes は Drop 時にゼロ化される。

ただし、`adapters/blob.rs` の `decrypt_secret_protected` において重大な漏れがある。

```
// blob.rs L66-70
let unwrapped_key = device.unwrap_key(&blob.wrapped_key)?;  // Vec<u8>、Zeroizingでない
if unwrapped_key.len() != CONTENT_KEY_LEN {
    bail!(...);
}
content_key.write_all(&unwrapped_key)?;  // write 後も unwrapped_key は生きている
```

`device.unwrap_key()` の戻り値 `Vec<u8>` は `Zeroizing` でラップされていない。`content_key`（`ProtectedInputBuffer`）へ `write_all` した後も、`unwrapped_key` のバッキングメモリがスタックフレーム or ヒープ上に平文のまま残る。Drop 時にゼロ化されない。

さらに `adapters/yubikey.rs` の `unwrap_key` 実装では、OAEP アンパッド結果を `write_oaep_unpadded_sha256` で `Vec<u8> output` へ書き込み `Ok(output)` で返す。この `output` は `Zeroizing` 非ラップの通常 `Vec<u8>` であるため、呼び出し元で Drop されてもゼロ化されない。

**修正が必要な箇所**:
- `ports.rs` の `SecretDevice::unwrap_key` 戻り値を `Zeroizing<Vec<u8>>` に変更する。
- `adapters/yubikey.rs` の `unwrap_key` 内の `output` を `Zeroizing::new(Vec::new())` に変更する。
- `adapters/blob.rs` の `unwrapped_key` が `Zeroizing<Vec<u8>>` で受け取られるようにする。

### PINのメモリ内保持

**合格**

YubiKey PIN の取得経路は `adapters/prompt.rs` の `read_yubikey_pin_raw()` に集約されており、PIN は `Zeroizing<Vec<u8>>` として返される。`application.rs` の `verify_pin_for_secret_reads` では `protect_bytes()` で `ProtectedSecret<'session>` へ変換してから `pin.with_secret(...)` の借用スコープ内でのみ `device.verify_pin(pin)` に渡す。平文 bytes はそのスコープを超えない。

ログ出力・エラーメッセージに PIN 値が含まれる箇所はなく、エラーメッセージは「YubiKey PIN must be 6 to 8 bytes」など抽象的な文言に限定されている。`validate_yubikey_pin` のエラーも PIN 値を含まない。

### エラーメッセージによる情報漏洩

**合格**

調査したすべてのファイルにおいて、エラーメッセージに secret 値・PIN 値・暗号鍵の内容が含まれていない。エラーは「failed to decrypt {name}」「{name} is not stored on this YubiKey」など、名前（`SecretName` の表示文字列）のみを含む。暗号処理の失敗は `map_err(|_| anyhow!(...))` でパディングエラーの詳細を捨てており、timing-leak 対策の意図が確認できる。

`SecretBlob` の `Debug` 実装は `<redacted:N bytes>` を明示しており、誤ってデバッグ出力に流れても平文が漏洩しない。

### secret値のclone/copy

**合格**

`ProtectedSecret` に `Clone`・`Copy` の derive や手動 impl が存在しない。`ProtectedInputBuffer` も同様。`SecretBytes = Zeroizing<Vec<u8>>` は `Zeroizing` の制約上 `Copy` を持たない。

`adapters/terminal.rs` で `line.clone()` が使われているが、これは yes/no 応答文字列であり secret ではない。`adapters/test_stub.rs` でも `config.clone()` が使われているが test double の設定構造体であり secret ではない。

### stdin/stdout境界

**合格**

`run_get_with` では `boundary.require_stdout_pipe()?` が `SecretSession::start()` および device open よりも前に呼び出されており、secret 復号前に TTY チェックが確実に行われる。

`write_secret_to_stdout` の実装（`adapters/stdout.rs`）でも `ensure_secret_stdout_not_terminal()` を冒頭で確認してから `terminal::write_all_stdout` を呼ぶ二重防衛になっている。

### EnrollmentBytes

**合格**

`ports.rs` の `EnrollmentBytes` 構造体は `bw_email`・`bw_password`・`bws_access_token` のすべてのフィールドを `Zeroizing<Vec<u8>>` で宣言している。

`adapters/enrollment_json.rs` の `parse_to_bytes` 実装では、各フィールドを `ProtectedSecret` として保護した後に `Zeroizing::new(secret.with_secret(|b| b.to_vec()))` で `EnrollmentBytes` へ変換しており、フィールド値が `Zeroizing` の外に漏れない。

`application.rs` の `read_enrollment_secret_set_from_user` では `EnrollmentBytes` の各フィールドを受け取り直ちに `protect_bytes()` で `ProtectedSecret<'session>` へ変換しており、`Zeroizing<Vec<u8>>` は変換後に Drop されゼロ化される。

## 総合判定

**不合格**

`adapters/blob.rs` の `decrypt_secret_protected` において、`device.unwrap_key()` が返す `Vec<u8>`（復号済みの content encryption key）が `Zeroizing` でラップされていない。この値は `content_key`（`ProtectedInputBuffer`）への `write_all` 後もヒープ上に平文のまま残り、Drop 時にゼロ化されない。

同様に `adapters/yubikey.rs` の `unwrap_key` 実装において、OAEP アンパッド結果を格納する `output: Vec<u8>` が `Zeroizing` 非ラップのまま返される。

`SecretDevice::unwrap_key` の戻り値型を `Zeroizing<Vec<u8>>` に変更し、関連する実装・呼び出し側を修正することでリスクは解消できる。他の観点（PINのメモリ保護・エラー情報漏洩・clone/copy禁止・stdout境界・EnrollmentBytes保護）については問題を確認できなかった。

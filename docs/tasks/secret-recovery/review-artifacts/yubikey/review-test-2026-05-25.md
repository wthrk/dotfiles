# テストレビュー記録 — YubiKey 秘密情報保存 (#12)

- レビュー担当: テストレビュー担当
- レビュー日: 2026-05-25
- 対象コード: 現行コード全体（git diff ではない）
- レビュー対象ファイル:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/storage_service.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/domain/model.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`

---

判定: 不合格

判定要約: `#[cfg(test)]` ラップの test double（`FakeBoundary`・`FakeDevice`）が production tree の `application/` 配下に残存している。作業定義文書の完了条件「production コードに test double が含まれない（V14, V15 の解消）」が直接違反されている。

## 根拠:

### 1. #[cfg(test)] 残存の確認（V14, V15 解消条件違反）

スキル規則「`#[cfg(test)]` ラップを用いたテストコードが production ファイル（`adapters/`・`application/` 等）に残存している場合は `判定: 不合格`」に照らして確認した。

**`rust/dotfiles-cli/src/secrets/application.rs` 行 621–1224**

`#[cfg(test)] mod tests` が存在し、その中に以下の test double が定義されている。

- `FakeBoundary`（`SecretsBoundary` を impl する偽実装）
- `FakeDevice`（`ports::SecretDevice` を impl する偽実装）
- `FakeDeviceState`（`FakeDevice` 内部状態）
- `protected_enrollment_secret_set`（test fixture helper）
- `make_fake_secret`（test fixture helper）

これらは `application/` 配下の production ファイルに `#[cfg(test)]` で存在しており、作業定義文書の完了条件「production コードに test double が含まれない（V14, V15 の解消）」に違反する。

**`rust/dotfiles-cli/src/secrets/application/storage_service.rs` 行 248–768**

`#[cfg(test)] mod tests` に `FakeDevice`（`SecretDevice` を impl する偽実装）が定義されている。これも `application/` 配下の production ファイルへの test double 混入であり、同じ違反に該当する。

**`rust/dotfiles-cli/src/secrets.rs` 行 45–58**

production struct `EnrollmentSecretSet` の `impl` ブロック内に `#[cfg(test)]` アノテーション付きメソッド `assert_secret_eq` が定義されている。これは production struct に test-only メソッドを埋め込む形式であり、同じ違反の一形態である。

**`rust/dotfiles-cli/src/secrets/adapters/yubikey.rs` 行 462–509**

`#[cfg(test)] mod tests` が存在し、`classify_empty_attempts_for_test` というヘルパー関数が定義されている。このヘルパーは test double ではないが、production ファイル内 `#[cfg(test)]` ブロックであることを記録する。ただしこれはアーキテクチャ規則上 `adapters/` 配下であり、違反種別の評価はスコープ外とする（本レビューはテストレビューのみを担当）。

### 2. テストによる完了条件の網羅確認

作業定義文書「完了の判定条件」のうち、振る舞いとして検証可能な条件について `tests/secrets_cli.rs` を直接確認した。

| 完了条件（振る舞い面） | カバーしているテスト | 状態 |
|---|---|---|
| setup: 未初期化では成功、初期化済みでは拒否 | `setup_runs_with_stub_yubikey`、`setup_rejects_initialized_stub_yubikey` | 確認済 |
| put: stdin pipe から保存、空入力の拒否、非対話で serial 必須 | `put_stores_non_tty_stdin_secret_with_stub_yubikey`、`put_rejects_empty_stdin_secret_with_stub_yubikey`、`put_rejects_non_tty_without_serial_with_stub_yubikey` | 確認済 |
| put: TTY prompt から保存 | `put_stores_tty_prompt_secret_with_stub_yubikey` | 確認済 |
| get: secret の取得成功、破損ストレージで失敗、TTY 出力拒否 | `get_outputs_seeded_stub_secret_with_stub_yubikey`、`get_fails_when_seeded_stub_storage_is_corrupt_with_stub_yubikey`、`get_refuses_secret_output_to_tty_with_stub_yubikey` | 確認済 |
| enroll-primary: stdin JSON 保存、不正 JSON 拒否、TTY prompt 保存 | `enroll_primary_stores_non_tty_stdin_json_with_stub_yubikey`、`enroll_primary_rejects_invalid_stdin_json_with_stub_yubikey`、`enroll_primary_stores_tty_prompt_secrets_with_stub_yubikey` | 確認済 |
| enroll-spare: stdin JSON 保存、同一 serial 拒否、primary 読み取り経路 | `enroll_spare_stores_non_tty_stdin_json_with_stub_yubikey`、`enroll_spare_rejects_same_primary_and_spare_serial_with_stub_yubikey`、`enroll_spare_uses_stub_yubikey_without_secret_reentry` | 確認済 |
| enroll-spare: TTY PIN 入力 | `enroll_spare_reads_yubikey_pins_from_pty_with_stub_yubikey` | 確認済 |
| rotate-bws-token: stdin pipe 保存、TTY prompt 保存、spare 同期、部分成功 JSON | `rotate_bws_token_stores_non_tty_stdin_secret_with_stub_yubikey`、`rotate_bws_token_stores_tty_prompt_secret_with_stub_yubikey`、`rotate_bws_token_updates_spare_after_tty_device_replacement_with_stub_yubikey`、`rotate_bws_token_emits_partial_success_json_when_replacement_fails_with_stub_yubikey` | 確認済 |
| rotate-bws-token: pipe stdin と TTY PIN の同時使用 | `rotate_bws_token_reads_pin_from_tty_while_token_comes_from_pipe_with_stub_yubikey` | 確認済 |
| verify-yubikey: local storage 検証、破損ストレージで失敗、TTY PIN 入力 | `verify_yubikey_checks_seeded_stub_storage_with_stub_yubikey`、`verify_yubikey_fails_when_seeded_stub_storage_is_corrupt_with_stub_yubikey`、`verify_yubikey_reads_yubikey_pin_from_pty_with_stub_yubikey` | 確認済 |
| verify-yubikey: 外部確認拒否（--all、--check、組み合わせ） | `verify_yubikey_rejects_all_flag_with_stub_yubikey`、`verify_yubikey_rejects_check_flag_with_stub_yubikey`、`verify_yubikey_rejects_all_and_check_combination_with_stub_yubikey` | 確認済 |

振る舞い面のテストカバレッジは適切であり、各コマンドの主要ユースケース・エラー経路・TTY/非TTY 分岐を網羅している。

ただし完了条件のうちアーキテクチャ規約への適合（V1〜V13）は振る舞いテストでは検証できない性質のものであり、これらは構造レビュー担当・仕様適合レビュー担当の判定に委ねる。本レビューはテスト担当として、V14・V15 の「test double の production tree 混入」を直接確認して不合格とする。

### 3. test double の配置

完了条件「test double は tests 層に分離すること」に照らして確認した。

`tests/secrets_cli.rs` は integration test として `tests/` 層に配置されており、test fixture 制御のために `dotfiles_cli_secrets_test_contract` crate を経由してスタブ YubiKey を注入している。この経路は tests 層内に閉じており、適切である。

しかし、上記「1.」に列挙した `application/` 配下の `#[cfg(test)]` ブロックに `FakeBoundary`・`FakeDevice` が残存している状態は解消されていない。

## 差戻し内容

実装実行担当へ差し戻す。以下の修正を完了してから再レビューを要求すること。

1. `rust/dotfiles-cli/src/secrets/application.rs` の `#[cfg(test)] mod tests` ブロック全体（`FakeBoundary`・`FakeDevice` 含む）を `tests/` 層へ移設する。
2. `rust/dotfiles-cli/src/secrets/application/storage_service.rs` の `#[cfg(test)] mod tests` ブロック全体（`FakeDevice` 含む）を `tests/` 層へ移設する。
3. `rust/dotfiles-cli/src/secrets.rs` の `EnrollmentSecretSet::assert_secret_eq`（`#[cfg(test)]` メソッド）を production struct から除去する。使用箇所がある場合は tests 層の代替手段へ置き換えること。

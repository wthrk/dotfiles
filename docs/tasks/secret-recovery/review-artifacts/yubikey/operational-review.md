# 運用レビュー記録

> 履歴専用記録: このファイルは過去サイクルのレビュー本文を保存するための記録であり、現行サイクルの判定対象外である。現行サイクルの正本は `review.md` と `confirmation.md` に一本化する。旧 harness 名・旧 path・複数サイクルの判定語が本文に残る場合も、履歴当時の記録として扱い、現行判定には使用しない。

- レビュー実施日: 2026-05-25
- 対象ブランチ: feat/yubikey-secret-storage
- HEAD: c581d2e8f835c750d4a105718e67e9f2785574b5
- 判定: 合格

## 確認項目ごとの結果

### 非対話実行時のエラーメッセージ

**結果: 合格**

`require_serial`、`require_option`、`require_stdin_pipe`、`require_stdin_json_pipe`、`require_stdout_pipe` のすべてが `RealSecretsBoundary` で実装されており、エラーメッセージは明確。

- `require_serial`: serial が None かつ非対話時 → `"pass --serial in non-interactive use"` / `"pass --primary-serial in non-interactive use"` / `"pass --spare-serial in non-interactive use"`（use case ごとに定数で分岐）
- `require_option`: option 未指定かつ非対話時 → `"pass {option_name} in non-interactive use"`（option 名を含む）
- `require_stdin_pipe`: stdin が TTY の場合 → `"--stdin requires pipe or redirect input"`
- `require_stdin_json_pipe`: `--stdin-json` 指定かつ stdin が TTY の場合 → `"--stdin-json requires pipe or redirect input"`
- `require_stdout_pipe`: stdout が TTY の場合 → `"refusing to write secret to terminal; redirect stdout to a file or pipe"`

各メッセージはどの option を渡すべきかを具体的に示しており、CI/CD ログからも診断できる内容になっている。

application 層の unit test（application.rs 内 `#[cfg(test)] mod tests`）では、非対話境界を `FakeBoundary::with_stdin_terminal(false)` で再現し、各 error メッセージを文字列一致で確認している（例: `"pass --serial in non-interactive use"`、`"--stdin requires pipe or redirect input"` 等）。

`run_enroll_primary_with` では `require_stdin_json_pipe` に加えて `require_option(options.stdin_json, "--stdin-json")` も呼ばれており、`--stdin-json` が非対話実行で必須であることを二重に確認している。この設計は意図的であるが、`require_stdin_json_pipe`（TTY 判定）と `require_option`（非対話フラグ判定）の役割が重複している点は設計的冗長であるものの、安全側に倒れており運用上の問題はない。

### ユーザーフィードバック（summary/report）

**結果: 合格**

`enroll-primary`、`enroll-spare`、`rotate-bws-token`、`verify-yubikey` の各 use case で、完了時に `boundary.write_report(&summary)` または `boundary.write_report(&summaries)` が呼ばれている。

- `enroll-primary`: `EnrollSummary`（serial、role、checks）を JSON 出力。`local_storage` check も含む。
- `enroll-spare`: 同上（`YubikeyRole::Spare`）。
- `rotate-bws-token`: 単一 device では `VerifySummary` を出力。複数 device では `Vec<VerifySummary>` を出力。途中失敗時は `PartialRotateBwsTokenSummary`（更新済み device のリスト）を出力し、再実行対象を利用者が判別できる。
- `verify-yubikey`: `VerifySummary` を出力。外部 check（`bws`、`bw-login`）が未実装の場合は、summary を出力してから error で失敗するため、どこまで完了したかが明確。

`write_report` は `RealSecretsBoundary` で `serde_json::to_string_pretty` を使って stdout へ出力する実装になっており、JSON 形式で機械処理も可能。

`put`・`setup`・`get` の低水準 command は summary を出力しないが、これらは低水準操作であり、成功時は exit code 0 のみで十分と判断できる。

### エラーの伝達

**結果: 合格**

`anyhow::Context` による error chain が適切に付いている。

- `storage_service.rs` の `read_secret_blob`: `"{} is not stored on this YubiKey"` / `"failed to decode {}"` と `with_context` でエラーに秘密名を付与。
- `enrollment_json.rs`: `read_enrollment_json_bytes` で `"failed to parse bootstrap secret JSON"` を context として付加。
- `adapters/real_boundary.rs`: `open_spare_device` で `"failed to install interrupt handler for spare YubiKey"` を context に付加。
- `storage_service.rs`: `check_setup_preconditions`、`check_put_target_writable` など操作前条件の失敗は具体的なメッセージを `bail!` で返している（例: `"YubiKey secret storage is already initialized"`、`"{} already exists; pass --force to replace it"`）。

manifest 不整合時のエラーメッセージ（`"YubiKey secret manifest does not match dotfiles secret-recovery format"`）も運用上の診断に十分な情報量がある。

### CLIオプションと処理の一貫性

**結果: 合格**

- `--serial` オプションは全 use case（`setup`、`put`、`get`、`enroll-primary`、`rotate-bws-token`、`verify-yubikey`）で `boundary.require_serial` に渡された後、`boundary.open_device(options.serial)` に引き渡されており一貫している。
- `--stdin` オプションは `put`・`rotate-bws-token` で `require_single_stdin_secret_source` を経由して pipe チェックを行い、`read_protected_secret_for_put` の分岐に渡される。
- `--stdin-json` オプションは `enroll-primary`・`enroll-spare` で `require_stdin_json_pipe` と `require_option` を経由し、`read_enrollment_secret_set_from_user` の分岐に渡される。
- `--force` は `put` のみで受け取り、`storage_service::put` / `check_put_preconditions` に直接渡されている。
- `--check` / `--all` は `verify-yubikey` で受け取り、`requested_external_checks` で変換後に処理される。`--all` と `--check` を同時指定した場合は `bail!` で明確に拒否。

`run_rotate_bws_token_with` の分岐は、`serial` 指定ありの場合と指定なしの場合で処理経路が分かれているが、両経路とも `require_serial(None, ...)` または serial 付き `open_device` に一貫してつながっている。

### 非対話モードの堅牢性

**結果: 合格**

TTY 判定は `adapters/terminal.rs` の `stdin_is_terminal()` / `stdout_is_terminal()` に集約されており、`std::io::IsTerminal` traitを使用している。この判定は `adapters/real_boundary.rs` の `RealSecretsBoundary` 内で行われ、application 層には TTY 判定が漏れていない。

`adapters/terminal.rs` の `prompt_yes_no`、`wait_for_enter`、`read_hidden_input`、`read_terminal_line_interruptible` はすべて非 TTY 時の動作を明示しており:
- `prompt_yes_no`: stdin が TTY でない場合 `false` を返す（停止せず継続）
- `wait_for_enter`: stdin が TTY でない場合は `noninteractive_error` でエラー
- `read_hidden_input`: `/dev/tty` へフォールバックして stdin payload を消費しない

CI/CD での実行では stdin が pipe になるため、`require_serial` / `require_option` / `require_stdin_json_pipe` で適切に弾かれるか、または `--serial` / `--stdin-json` を明示指定する経路が使われる。`prompt_continue_rotation` は非 TTY 時に `false` を返すため、自動実行で無限待機に入らない設計になっている。

`InterruptGuard` によるシグナル処理も `open_spare_device` と `prompt_continue_rotation` で適切に使われており、Ctrl-C による中断も安全に処理できる。

## 総合判定

**合格**

運用上の主要観点（非対話実行時のエラーメッセージの明確さ、ユーザーフィードバック出力、エラー伝達の適切さ、CLIオプションの一貫性、非対話モードの堅牢性）のすべてで問題は見つからなかった。

- 非対話実行時のエラーメッセージは具体的な option 名を含み診断に十分
- `write_report` による JSON summary 出力が各 use case 完了時に確実に実施されている
- `anyhow::Context` によるエラーチェインが適切に付いており、秘密名や操作対象を含む
- CLIオプションから処理への受け渡しに一貫性がある
- TTY 判定は adapter 境界に閉じており、application 層が具体的な I/O 状態に依存していない
- 途中失敗時にも `PartialRotateBwsTokenSummary` として再実行支援情報を出力する設計が整っている

特記事項として、`run_enroll_primary_with` における `require_stdin_json_pipe` と `require_option` の二重チェックは設計的冗長だが、安全側に倒れており運用上の問題はない。外部 check（`bws`、`bw-login`）が未実装であることは summary に `failed` として明示してから error で終了するため、利用者は状態を把握できる。

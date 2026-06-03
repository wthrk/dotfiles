# Bitwarden Password Manager 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Password Manager` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `完了`
- 対象差分識別子: `main..feat/secrets-bw-login-issue-16`
- 対象ブランチ: `feat/secrets-bw-login-issue-16`
- 確認開始時 HEAD: `1318b19`
- 差分区分: `実装`

## 確認手順と結果

- 手順: リポジトリ CI ゲート `nix develop -c cargo xtask check static` を実行（fmt / `RUSTFLAGS=-D warnings cargo check --workspace` / clippy `--workspace --all-targets -- -D warnings` / `cargo test --workspace --all-targets` / `cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli` / `cargo test -p dotfiles-cli --lib secrets::application` / nix 静的検査）。
- 結果: 是正後の再実行で `all checks passed!`（全実装単位テスト・clippy・fmt・nix 検査が合格）。
- 未実施理由（未実施がある場合）: `該当なし`

## 実装進捗への影響

- 対象コードパス差分: `差分あり`（新規: `secrets/domain/bw_login.rs`・`secrets/application/run_bw_login.rs`・`secrets/adapters/bw/login_adapter.rs`・`secrets/adapters/bw/login_stub.rs`・`secrets/support/protection/bw_login.rs`、変更: `secrets.rs`・`entrypoint/dispatch.rs`・`application/run_verify_yubikey_with.rs`・`ports/bw.rs`・`ports/io.rs`・`adapters/io/*`・`support/protection.rs`・`domain/commands.rs`・`tests/secrets_cli.rs`、文書: `README.md`）。
- 文書整合メモ: README に `bw-login` の使用法と primary/spare の manual login validation 手順を追記。canonical spec は既に bw-login を規定済みのため改変なし。
- 前進可否メモ（確認 / レビュー / 実装状態）: 確認完了・集約レビュー合格・実装済み。コミット/PR（Closes #16）へ前進可。

## セキュリティ確認結果

- 秘密値/認証情報の露出確認: `完了`（master password は `ProtectedSecret` のまま port へ渡り、平文は borrow 境界内のみ・子プロセス `BW_PASSWORD` env 限定）
- ログ/引数/一時ファイル/stdout/stderr 確認: `完了`（argv/ログ/一時ファイル/親環境へ secret を残さない。login stdout/stderr 破棄、unlock は `--raw` のみ capture、stub observation に master password 非出力）
- 権限境界/永続化/失敗時挙動確認: `完了`（失敗時に session 非返却・report 非記述、`BW_SESSION` 非永続、verify 経路は session 破棄、直接 libc 不使用）
- 未実施理由（未実施がある場合）: `該当なし`

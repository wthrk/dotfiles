# Bitwarden Secrets Manager 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Secrets Manager` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `進行中`
- 対象差分識別子: `bws-design-pr-2026-05-28-5d6e495`
- 対象ブランチ: `copilot/bitwarden-secrets-manager-client`
- 確認開始時 HEAD: `5d6e495`
- 差分区分: `実装`

## 確認手順と結果

- 手順: `cargo check` および `cargo test -p dotfiles-cli` を実行
- 結果: `cargo check` 成功 / `cargo test` 全 106 件 passed
- 未実施理由（未実施がある場合）: なし

## 実装進捗への影響

- 対象コードパス差分:
  - `rust/dotfiles-cli/src/secrets/domain/values.rs` — `BwsSecretName`、`RestoreGpgCommand`、`RestorePassCommand` 追加
  - `rust/dotfiles-cli/src/secrets/ports.rs` — `BwsClientPort` trait 追加
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs` — BWS check 実装（`BwsClientPort` 経由でトークン読み出し＋両 BWS secret fetch）
  - `rust/dotfiles-cli/src/secrets/application/run_restore_gpg_with.rs` — 新規（stub: fetch 成功後 GPG import は未実装）
  - `rust/dotfiles-cli/src/secrets/application/run_restore_pass_with.rs` — 新規（stub: fetch 成功後 pass clone は未実装）
  - `rust/dotfiles-cli/src/secrets/application.rs` — 新規 module 宣言追加
  - `rust/dotfiles-cli/src/secrets.rs` — `RestoreGpg`/`RestorePass` CLI command、`BwsClientPort` bound 追加
  - `rust/dotfiles-cli/src/secrets/adapters/bws_client.rs` — 新規 stub adapter
  - `rust/dotfiles-cli/src/secrets/adapters.rs` — `BwsClientAdapter` フィールド、`BwsClientPort` impl 追加
  - `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs` — `BwsClientPort` mock impl 追加
- 文書整合メモ: `tasks.md` の作業状態を `進行中` へ更新済み
- 前進可否メモ: デザインPR 完了。BWS SDK 統合（adapter 本実装）と restore-gpg/pass の完全実装は次サイクル。レビュー受け付け可能状態。

## セキュリティ確認結果

- 秘密値/認証情報の露出確認: `完了` — `SecretMaterial` 境界を通じてのみ access_token を受け渡しており、raw bytes は ports/adapters 外へ露出しない。stub adapter は受け取った値を使わず破棄する。
- ログ/引数/一時ファイル/stdout/stderr 確認: `完了` — 新規コードにログ出力・println! なし。fetch_bws_secret の引数は SecretMaterial 型で保護されている。
- 権限境界/永続化/失敗時挙動確認: `完了` — stub adapter は無条件で anyhow::bail! を返す。BWS fetch 失敗時は VerifySummary に CheckStatus::Failed を記録してエラーを返す。永続化なし。

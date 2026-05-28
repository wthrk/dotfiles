# Bitwarden Secrets Manager 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Secrets Manager` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `進行中`
- 判定位置づけ: `デザインPR段階の確認継続中（完了判定ではない）`
- 対象差分識別子: `bws-design-pr-current-cycle`
- 対象ブランチ: `copilot/bitwarden-secrets-manager-client`
- 確認開始時点参照: `work-items/bitwarden-secrets-manager.md` 記載の `実装/テスト差分の保存コミット終端`
- 差分区分: `実装`

## 確認手順と結果

- 手順: `cargo check` および `cargo test -p dotfiles-cli` を実行（`verify-yubikey --check bws` は未実施）
- 結果: `cargo check` 成功 / `cargo test` 全 106 件 passed / `verify-yubikey --check bws` の証跡は未取得
- 未実施理由（未実施がある場合）: `../../work-items/bitwarden-secrets-manager.md` の完了条件で必須とされる `verify-yubikey --check bws` の確認証跡を未記録のまま、確認完了扱いになっていたため。完了条件と証跡の整合を優先して `確認状態` を `進行中` へ是正。

## 実装進捗への影響

- 対象コードパス差分:
  - `rust/dotfiles-cli/src/secrets/domain/values.rs` — `BwsSecretName`、`RestoreGpgCommand`、`RestorePassCommand` 追加
  - `rust/dotfiles-cli/src/secrets/ports.rs` — `BwsClientPort` trait 追加
  - `rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs` — BWS check 実装（`BwsClientPort` 経由でトークン読み出し＋両 BWS secret fetch）
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_prompt.rs` — BWS token rotate prompt 経路
  - `rust/dotfiles-cli/src/secrets/application/run_rotate_bws_token_with_stdin.rs` — BWS token rotate stdin 経路
  - `rust/dotfiles-cli/src/secrets/application.rs` — 新規 module 宣言追加
  - `rust/dotfiles-cli/src/secrets.rs` — BWS 関連 command ルーティングと `BwsClientPort` bound 追加
  - `rust/dotfiles-cli/src/secrets/adapters.rs` — `BwsClientAdapter` フィールド、`BwsClientPort` impl 追加
  - `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs` — `BwsClientPort` mock impl 追加
- 文書整合メモ: `docs/tasks/secret-recovery/tasks.md` の固定実装単位トラッカー `確認` を `進行中` へ是正し、完了条件未充足の状態と一致させた。
- 前進可否メモ: デザインPR 完了。BWS SDK 統合（adapter 本実装）と restore-gpg/pass の完全実装は次サイクル。加えて、`verify-yubikey --check bws` の確認証跡取得とレビュー個別判定の充足まで `確認`/`レビュー` は完了扱いにしない。

## セキュリティ確認結果

- 秘密値/認証情報の露出確認: `完了` — `SecretMaterial` 境界を通じてのみ access_token を受け渡しており、raw bytes は ports/adapters 外へ露出しない。stub adapter は受け取った値を使わず破棄する。
- ログ/引数/一時ファイル/stdout/stderr 確認: `完了` — 新規コードにログ出力・println! なし。fetch_bws_secret の引数は SecretMaterial 型で保護されている。
- 権限境界/永続化/失敗時挙動確認: `完了` — stub adapter は無条件で anyhow::bail! を返す。BWS fetch 失敗時は VerifySummary に CheckStatus::Failed を記録してエラーを返す。永続化なし。

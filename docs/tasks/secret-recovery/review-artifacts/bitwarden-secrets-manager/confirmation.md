# Bitwarden Secrets Manager 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Secrets Manager` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `完了`
- 判定位置づけ: `デザインPR段階 current-cycle 差分の確認完了（作業項目全体の完了判定ではない）`
- 対象差分識別子: `bws-design-pr-current-cycle`
- 対象ブランチ: `copilot/bitwarden-secrets-manager-client`
- 確認開始時点参照: `work-items/bitwarden-secrets-manager.md` 記載の `実装/テスト差分の保存コミット終端`
- 差分区分: `実装`

## 確認手順と結果

- 手順:
  - `direnv exec . cargo check -p dotfiles-cli`
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli verify_yubikey_runs_bws_external_check`
- 結果:
  - `cargo check -p dotfiles-cli` 成功
  - `verify_yubikey_runs_bws_external_check` passed（`verify-yubikey --serial 2001 --check bws` 実行経路が `secrets_cli` integration test 内で成功）
- 未実施理由（未実施がある場合）: `なし`

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
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/mod.rs` — `piv_io` の責務分割後の共通境界定義へ再構成
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/device_selection.rs` — device selection 責務を分離
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/process_io_adapter.rs` — process I/O port 翻訳責務を分離
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/storage_adapter.rs` — storage port 翻訳責務を分離
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/report_adapter.rs` — JSON report 変換責務を分離
  - `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs` — `BwsClientPort` mock impl 追加
- 文書整合メモ: `docs/tasks/secret-recovery/tasks.md` の固定実装単位トラッカー `確認` と本記録の状態を `完了` で同期し、required evidence 反映済みの current-cycle 記録として扱う。
- 前進可否メモ: `verify-yubikey --check bws` を含む required evidence は確認済み。review.md の必須8役割判定は全件 `合格` で集約済み。

## セキュリティ確認結果

- 秘密値/認証情報の露出確認: `完了` — `BwsClientAdapter` は `bws-access-token` を `Zeroizing<Vec<u8>>` / `Zeroizing<String>` で一時展開し、破棄時消去を保証する。stub adapter は受け取った値を使わず破棄する。
- ログ/引数/一時ファイル/stdout/stderr 確認: `完了` — `bws` 実行失敗時の user-visible error は固定要約（secret 名 + exit status）のみを返し、raw stderr を埋め込まない。
- 権限境界/永続化/失敗時挙動確認: `完了` — `secrets-internal-test-stub` 有効時の stub adapter は `fetch_bws_secret` で secret 名ごとの固定値を `Ok(...)` 返却し、access token は受け取って破棄する。非 stub 経路では BWS fetch 失敗時に VerifySummary へ `CheckStatus::Failed` を記録してエラーを返す。永続化なし。

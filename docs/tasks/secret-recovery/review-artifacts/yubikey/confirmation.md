# YubiKey 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `YubiKey` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `実施済み（要修正あり）`
- 対象差分識別子: `working tree (base: 2e5c7cc, 2026-05-24 20:03:09 +0900)`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 確認開始時 HEAD: `2e5c7cc`
- 差分区分: `実装`

## 確認手順と結果

- 手順:
  - `direnv exec . cargo fmt --all`
  - `direnv exec . cargo xtask check`
- 結果:
  - `cargo fmt --all`: 成功
  - `cargo xtask check`: 失敗（`cargo test -p dotfiles-cli --features secrets-test-stub --test secrets_cli` で 1 件失敗）
  - 失敗テスト: `rotate_bws_token_reads_pin_from_tty_while_token_comes_from_pipe_with_stub_yubikey`
  - 失敗内容: `failed to open controlling terminal`
- 未実施理由（未実施がある場合）: `なし`

## 実装進捗への影響

- 対象コードパス差分: `差分あり`
- 文書整合メモ: `確認証跡を本ファイルに追記`
- 前進可否メモ（確認 / レビュー / 実装状態）: `確認は実施済みだが検証失敗 1 件が残存。レビュー着手前に修正が必要。`

## セキュリティ確認結果

- 秘密値/認証情報の露出確認: `実施（平文秘密値・鍵素材・トークンの新規永続化/ログ出力なし）`
- ログ/引数/一時ファイル/stdout/stderr 確認: `実施（失敗ログは制御端末のオープン失敗のみで、秘密値露出なし）`
- 権限境界/永続化/失敗時挙動確認: `実施（terminal 入力境界の移設後、制御端末取得失敗時に異常終了する経路を確認）`
- 未実施理由（未実施がある場合）: `なし`

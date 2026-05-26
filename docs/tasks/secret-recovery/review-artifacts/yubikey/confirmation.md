# YubiKey 確認記録

この文書は `docs/tasks/secret-recovery/work-items/yubikey.md` の現行サイクル確認証跡（current worktree 基準）である。

## 現行サイクル（2026-05-26）

- 確認状態: `実施済み（要修正）`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 対象差分識別子: `yubikey-current-cycle-2026-05-26-head-d32848a`
- 確認基準: `current worktree を正本とする`
- 対象スコープ:
  - `rust/dotfiles-cli/src/secrets/application.rs`
  - `rust/dotfiles-cli/src/secrets/application/run_*.rs`
  - `rust/dotfiles-cli/src/secrets/ports.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/device.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/secret_io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/report.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/domain.rs`
  - `rust/dotfiles-cli/src/secrets/domain/model.rs`
  - `rust/dotfiles-cli/src/secrets/domain/wire.rs`
  - `rust/dotfiles-cli/src/secrets/support.rs`
  - `rust/dotfiles-cli/src/secrets/support/aead.rs`
  - `rust/dotfiles-cli/src/secrets/support/oaep.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/buffer.rs`
  - `rust/dotfiles-cli/tests/secrets_cli.rs`

## 実行コマンド

- `direnv exec . cargo check -p dotfiles-cli`
- `direnv exec . cargo test -p dotfiles-cli --features secrets-test-stub --test secrets_cli --no-run`

## 結果要約

- `cargo check`: 実行済み
- `cargo test --no-run`: 実行済み
- 判定: `要修正`

## 2026-05-26 追加実装サイクル追記

- 未解決 1,2,3,4,5,6,7,9 のコード是正を反映した。
- `device_test_stub.rs` の書き込みイベントは `<redacted>` を出力しており、stub 側 plaintext stderr 出力は現 current worktree では再現していない。
- 未解決 10 は本追記を含め review/confirmation/work-item の同期を再実施した。
- 判定前提更新: `rust/dotfiles-cli/src/secrets/adapters/piv_io/device_test_stub.rs` は PIV/YubiKey 固有 concrete 実装として評価し、tests 層固定の一般論は適用しない。
- 判定前提更新: same-route 維持を前提に、`--test-stub-yubikey` / `yubikey_runtime` / 別 binary / 別 CLI / command-scenario branching / port-boundary swap は採用しない。
- 判定前提更新: secret 本文は `ProtectedSecret` 型以外で扱わず、`rust/dotfiles-cli-secrets-test-stub/` は復活させない。

## ブロッカー要約

- review artifact 間で verdict の整合が崩れていたため、現行サイクルの判定を `要修正` に統一した。
- 履歴レビュー内に現行 code path と不一致な参照が混在していたため、現行実在パスへ更新した。

## 前進可否

- 前進可否: `前進不可（差し戻し継続）`
- 理由: `review.md` の現行サイクル集約判定が `要修正` のため。

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
  - `rust/dotfiles-cli-secrets-test-contract/src/lib.rs`
  - `rust/dotfiles-cli-secrets-test-stub/src/device.rs`
  - `rust/dotfiles-cli-secrets-test-stub/src/dotfiles_main.rs`
  - `rust/dotfiles-cli-secrets-test-stub/src/main.rs`

## 実行コマンド

- `direnv exec . cargo check -p dotfiles-cli`
- `direnv exec . cargo test -p dotfiles-cli --test secrets_cli --no-run`

## 結果要約

- `cargo check`: 実行済み
- `cargo test --no-run`: 実行済み
- 判定: `要修正`

## ブロッカー要約

- review artifact 間で verdict の整合が崩れていたため、現行サイクルの判定を `要修正` に統一した。
- 履歴レビュー内に現行 code path と不一致な参照が混在していたため、現行実在パスへ更新した。

## 前進可否

- 前進可否: `前進不可（差し戻し継続）`
- 理由: `review.md` の現行サイクル集約判定が `要修正` のため。

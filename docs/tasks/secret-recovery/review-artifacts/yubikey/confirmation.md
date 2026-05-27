# YubiKey 確認記録

この文書は `docs/tasks/secret-recovery/work-items/yubikey.md` の現行サイクル確認証跡（current worktree 基準）である。

## 現行サイクル（2026-05-26）

- 確認状態: `実施済み（再レビュー待ち）`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 対象差分識別子: `yubikey-current-cycle-2026-05-26-base-ad92152`
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
- `direnv exec . cargo xtask check`
- `direnv exec . cargo clippy --workspace --all-targets`
- `direnv exec . env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`

## 結果要約

- `cargo check`: 実行済み
- `cargo test --no-run`: 実行済み
- `cargo xtask check`: 実行済み（`8740b1a`）
- `cargo clippy --workspace --all-targets`: 実行済み（`8740b1a`）
- `RUSTFLAGS='-D warnings' cargo test --workspace --all-targets`: 実行済み（`8740b1a`）
- 判定: `再レビュー待ち`

## 2026-05-26 ad92152 基準 current-cycle 追記

- current-cycle 基準コミット: `ad92152 refactor(secrets): align yubikey storage boundaries`
- 追加保存コミット: `f6d5d7c fix(secrets): keep pin secret access inside protection`
- 追加保存コミット: `022c21b fix(secrets): resolve yubikey review blockers`
- 確認証跡同期コミット: `8740b1a docs(secrets): sync yubikey current cycle commit`
- 現行状態: `再レビュー待ち`
- 確認前提: security は pass 済み。structural / operational Fail 修正は `022c21b` で反映済みであり、合格とは記録しない。
- `ad92152` 以降の変更ファイル集合:
  - `rust/dotfiles-cli/src/secrets.rs`
  - `rust/dotfiles-cli/src/secrets/adapters.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/device.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/report.rs`
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection.rs`
  - `rust/dotfiles-cli/src/secrets/support/protection/yubikey_pin.rs`
  - `docs/tasks/secret-recovery/tasks.md`
  - `docs/tasks/secret-recovery/work-items/yubikey.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/confirmation.md`
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/review.md`

## 2026-05-26 追加実装サイクル追記

- 未解決 1,2,3,4,5,6,7,9 のコード是正を反映した。
- `device_test_stub.rs` の書き込みイベントは `<redacted>` を出力しており、stub 側 plaintext stderr 出力は現 current worktree では再現していない。
- 未解決 10 は本追記を含め review/confirmation/work-item の同期を再実施した。
- 判定前提更新: `rust/dotfiles-cli/src/secrets/adapters/piv_io/device_test_stub.rs` は PIV/YubiKey 固有 concrete 実装として評価し、tests 層固定の一般論は適用しない。
- 判定前提更新: same-route 維持を前提に、`--test-stub-yubikey` / `yubikey_runtime` / 別 binary / 別 CLI / command-scenario branching / port-boundary swap は採用しない。
- 判定前提更新: secret 本文は `ProtectedSecret` 型以外で扱わず、`rust/dotfiles-cli-secrets-test-stub/` は復活させない。

## ブロッカー要約

- review artifact 間で verdict の整合が崩れていたため、review verdict は `要修正` を維持し、task/current-cycle 状態は `再レビュー待ち` として同期した。
- 履歴レビュー内に現行 code path と不一致な参照が混在していたため、現行実在パスへ更新した。

## 前進可否

- 前進可否: `前進不可（差し戻し継続）`
- 理由: `review.md` の現行サイクル集約判定は再レビュー前の `要修正` を維持しているため。

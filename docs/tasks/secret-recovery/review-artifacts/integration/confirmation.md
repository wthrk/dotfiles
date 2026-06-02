# 新規マシン復旧フロー統合 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `新規マシン復旧フロー統合` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `実施済み`
- 対象差分識別子: `base 1318b19 (origin/main 終端) .. working tree`（branch `feat/secrets-recovery-flow-integration-issue-17`）
- 対象ブランチ: `feat/secrets-recovery-flow-integration-issue-17`
- 確認開始時 HEAD: `1318b19`
- 差分区分: `実装`（実コード差分 + 対応する unit / stub integration test）

## 規約計画

- 適用規約: secret-recovery-spec.md（到達仕様の復旧フロー 手順1〜9 L104-112 / 停止条件 L194-214 / bw-login コマンド契約 L176-178 / verify-yubikey 外部確認 L147-157, L201）、secret-handling.md（master password を `BW_PASSWORD` env で子プロセスにだけ渡し保存しない、平文を argv/log/env/一時ファイル/stdout に残さない、外部処理は protection 内操作で完了）、hexagonal-implementation-rules.md（application=順序制御のみ / port=capability 契約 / adapter=翻訳 / support/protection=外部処理境界 backend）、integration.md（構造完了条件・境界維持の観点・レビュー合格条件）。
- #11 系粗粒度進捗との対応: #17 は #12〜#17 を新規マシン復旧フローとして統合する close-out 項目。手順1〜8（enroll/verify/restore-gpg/export-ssh/restore-pass/gpg-backup/pass-remote）は既に個別結線済み。手順9（bw-login）と `verify-yubikey --check bw-login` が現行 base に未結線であることを観察で確認。

## 実装計画（観察に基づく）

- 現行コード観察: `secrets.rs`(composition root) → `entrypoint/dispatch.rs`(command 分岐) → `application/run_*.rs`(use case) → `ports/*` → `adapters/*` の層構造を確認。`SecretsCommand` に `BwLogin` 不在、`run_bw_login` use case 不在、`BwLoginPort` 不在、`run_verify_yubikey_with` の `CheckName::BwLogin` 分岐は "not implemented yet" の固定エラーを返していた。base には #16 が未マージ（branch `feat/secrets-bw-login-issue-16` は base と同一 commit で実装なし）。
- 結線対象と更新順序: (1) domain `bw_login.rs`(summary) + `commands.rs`(BwLoginCommand) 追加 → (2) ports `bw_login.rs`(BwLoginPort) + io ports(BwLoginOtpInputPort / BwLoginEmailOverridePort / write_bw_login_report) 追加 → (3) application `run_bw_login.rs` 追加（restore-pass と同じ device→pin→storage→外部処理→report の順序制御）と `run_verify_yubikey_with` の bw-login check 結線 → (4) adapters `bw_login.rs`(+internal_stub) と io adapter / report adapter 実装 → (5) support/protection `bw_login.rs`(`bw login`/`bw unlock` を `BW_PASSWORD` env 借用境界で実行) と `protect_public_bytes`/`with_secret_utf8` 追加 → (6) entrypoint dispatch / composition root 結線 → (7) integration test 追加。
- integration.md 構造完了条件との対応: 個別 use case 結線は `application/run_bw_login.rs` と `run_verify_yubikey_with` の `application` 層で完結。外部依存境界（YubiKey storage / `bw` CLI 外部 command / 端末入力）は port 契約で分離し責務混在なし。停止条件（login/unlock 失敗・到達不可）を application 順序制御で一貫させた。

## 規約文書更新

- spec の bw-login コマンド契約・verify-yubikey 外部確認・復旧フロー手順9 は既に正本に定義済みであり、実装はその契約へ適合させた。重複定義回避のため spec への追記は不要と判断（`コード差分に従属する文書整合は本確認記録の更新に限定`）。

## 確認手順と結果

dev shell（`direnv exec .`）内で実行。

- `cargo build -p dotfiles-cli`（default = gpg-backend）: 成功。
- `cargo build -p dotfiles-cli --features secrets-internal-test-stub`: 成功。
- `cargo clippy -p dotfiles-cli --all-targets -- -D warnings`: 成功（警告 0）。
- `cargo clippy -p dotfiles-cli --all-targets --features secrets-internal-test-stub -- -D warnings`: 成功（警告 0）。
- `cargo fmt --check`: 成功（fmt 適用後 diff 0）。
- `cargo test -p dotfiles-cli --lib`: 205 passed / 0 failed。
- `cargo xtask check`: 全段階成功（fmt / check / clippy / test 205 unit / stub integration 42 / application route 64 / shell / workflows / nil / flake.lock / nix fmt / nix flake check = all checks passed）。
- 既存コマンド退行確認: restore-gpg / restore-pass / pass-remote / rotate-bws-token / verify-yubikey(bws) の stub integration test が全て成功（退行なし）。
- 未実施理由: なし。

## 実装進捗への影響

- 対象コードパス差分: `差分あり`（14 ファイル変更 + 6 ファイル新規。`git diff --stat` は実装記録参照）。
- 文書整合メモ: spec は既存契約へ適合。本確認記録のみ更新。
- 前進可否メモ（確認 / レビュー / 実装状態）: 実コード差分が存在し確認証跡が揃ったため `確認` を `実施済み` とする。`レビュー` / `実装状態` / 台帳前進は進捗判定担当・レビュー担当の責務であり本記録では前進記入しない。

## セキュリティ確認結果

- 秘密値/認証情報の露出確認: master password は `support/protection/bw_login` の `with_secret_utf8` 借用境界内でだけ `BW_PASSWORD` env value へ複製し `bw` 子プロセスへ渡す。借用 closure を抜けると複製は `Zeroizing` で破棄。`BW_PASSWORD` を argv へ載せず、`bw unlock --raw` の stdout(`BW_SESSION`) を application へ返さない。bw-email / OTP は非秘匿だが email は carrier 型統一のため protection 境界で保護値化（`protect_public_bytes`）し、生値取り出し API は追加していない。
- ログ/引数/一時ファイル/stdout/stderr 確認: secret 平文を CLI 引数・ログ・一時ファイル・永続環境変数・stdout/stderr に残さない。report は login/unlock の成立 bool（`logged_in`/`unlocked`）のみ出力し secret を含まない。
- 権限境界/永続化/失敗時挙動確認: login / unlock のいずれか失敗時は report を書かずに停止（`run_bw_login` の test で検証）。`--check bw-login` 到達不可時は Failed として report し error を伝播（停止条件 spec L201）。internal stub は実 `bw` CLI を起動せず、port 間 state を共有しない独立 stub。
- 未実施理由: なし。

## #16 依存により interface 結線にとどめた箇所

- 本ブランチに #16（bw-login, branch `feat/secrets-bw-login-issue-16`）は未マージ。手順9 と `--check bw-login` は spec のコマンド契約（L176-178, L201, L155）に対して application 層・port 契約・entrypoint で結線した。
- `support/protection/bw_login.rs` の実 `bw login` / `bw unlock` 実行（`BW_PASSWORD` env 受け渡し・`BW_SESSION` 取り回し・OTP method 3）は spec 契約どおりに `std::process` で実装したが、実 `bw` CLI バイナリに対する end-to-end 検証は #16 の責務であり、本統合では internal stub による CLI 経路の結線・順序・停止条件の検証にとどめる。#16 マージ時にこの protection 内操作と adapter が #16 の実装と整合するか確認する必要がある。

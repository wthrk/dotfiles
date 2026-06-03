# 新規マシン復旧フロー統合 確認記録

この文書は、`docs/tasks/secret-recovery/tasks.md` の作業項目 `新規マシン復旧フロー統合` に対する固定実装単位 `確認` の証跡である。

## 状態

- 確認状態: `実施済み`
- 対象差分識別子: `base 1318b19 (origin/main 終端) .. working tree`（branch `feat/secrets-recovery-flow-integration-issue-17`）
- 対象ブランチ: `feat/secrets-recovery-flow-integration-issue-17`
- 確認開始時 HEAD: `1318b19`
- 差分区分: `実装`（実コード差分 + 対応する unit / stub integration test）

## 規約計画

- 適用規約: secret-recovery-spec.md（`## 到達仕様の復旧フロー` 手順1〜9 / `## 停止条件` 節 / `### dotfiles secrets bw-login` コマンド契約 / `### dotfiles secrets verify-yubikey` 節の外部確認記述および `## 停止条件` 節の `--check bw-login` 到達確認項）、secret-handling.md（master password を `BW_PASSWORD` env で子プロセスにだけ渡し保存しない、平文を argv/log/env/一時ファイル/stdout に残さない、外部処理は protection 内操作で完了）、hexagonal-implementation-rules.md（application=順序制御のみ / port=capability 契約 / adapter=翻訳 / support/protection=外部処理境界 backend）、integration.md（構造完了条件・境界維持の観点・レビュー合格条件）。
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
- 権限境界/永続化/失敗時挙動確認: login / unlock のいずれか失敗時は report を書かずに停止（`run_bw_login` の test で検証）。`--check bw-login` 到達不可時は Failed として report し error を伝播（spec `## 停止条件` 節の `--check bw-login` 到達確認項）。internal stub は実 `bw` CLI を起動せず、port 間 state を共有しない独立 stub。
- 未実施理由: なし。

## #16 依存により interface 結線にとどめた箇所

- 本ブランチに #16（bw-login, branch `feat/secrets-bw-login-issue-16`）は未マージ。手順9 と `--check bw-login` は spec のコマンド契約（`### dotfiles secrets bw-login` 節 / `### dotfiles secrets verify-yubikey` 節の `--check bw-login` 外部確認記述 / `## 停止条件` 節の `--check bw-login` 到達確認項）に対して application 層・port 契約・entrypoint で結線した。
- `support/protection/bw_login.rs` の実 `bw login` / `bw unlock` 実行（`BW_PASSWORD` env 受け渡し・`BW_SESSION` 取り回し・OTP method 3）は spec 契約どおりに `std::process` で実装したが、実 `bw` CLI バイナリに対する end-to-end 検証は #16 の責務であり、本統合では internal stub による CLI 経路の結線・順序・停止条件の検証にとどめる。#16 マージ時にこの protection 内操作と adapter が #16 の実装と整合するか確認する必要がある。

## 差し戻し remediation 追記（PR #42 AI レビュー findings 対処）

確認開始時 HEAD `512b730` を起点に、PR #42 への AI レビュー findings 5 件を対処した。差分は `実装`（実コード差分 + test）。

### finding ごとの対処

- FINDING 1（P1 / セキュリティ / `support/protection/bw_login.rs`）: `bw login` / `bw unlock --raw` の `Command` に `.stdout(Stdio::null())` を追加（行80 / 行101）。`bw unlock --raw` が stdout へ出す `BW_SESSION`（および `bw login` の stdout）が dotfiles プロセスの stdout（端末・ログ・JSON report と同一ストリーム）へ継承漏洩する経路を閉じ、成立確認は exit status のみで行う。`BW_SESSION` を読まず・返さず・一時ファイル/永続 env へ出さない。stderr は `bw` 自身の診断出力（secret を含まない。`--raw` の session は stdout 限定）として失敗診断の可視化のため継承する旨を module doc に明記。
- FINDING 2（P2 / 出力破壊 / `support/protection/bw_login.rs` `check_reachable`）: reachability の `bw --version` にも `.stdout(Stdio::null())` を追加（行58）。version 文字列が `verify-yubikey` の JSON report 前に混入し machine-readable JSON を壊す経路を閉じた。
- FINDING 3（P2 / 仕様適合）: **方針 (a)+(b)+(c) を採用**（CLI 起動可能性確認に範囲を狭め、差分を本記録へ記載）。根拠: spec `## 停止条件` 節の `--check bw-login` 到達確認項が要求する Bitwarden Password Manager への真のサービス到達確認（server URL 設定・ネットワーク疎通）は実 `bw` 統合（#16）の責務であり、#17 単独で完結できない。`bw --version` は CLI バイナリ起動可能性のみを確認する。よって port 契約 `ports/bw_login.rs` の `check_bw_login_reachable` doc と `support/protection/bw_login.rs` の `check_reachable` doc を「CLI invocation capability 確認」に正確に狭め、spec の当該サービス到達確認との差分を既知の制約として両 doc から本記録へ参照させた。**既知の制約**: 現状の `--check bw-login` はネットワーク断・server URL 誤設定でも `bw` バイナリさえ起動できれば `ok` と報告する。真のサービス到達確認（例: `bw status` / server 設定確認）は #16 で実装し、その際に port 契約 doc を再度サービス到達確認へ広げる。
- FINDING 4（doc / `adapters/io/process.rs:119-122付近`）: OTP コメントを「dotfiles 自身の argv には載せず stdin から読む。後段で `bw login --code <otp>` の子プロセス argv には載るが、ワンタイムコードで長期 secret ではないため protection 保護値ではなく素の `String` で受け渡す」に修正し、誤解（argv へ一切載らない）を解消。
- FINDING 5（doc / `support/protection/bw_login.rs:25-26付近`）: `login_and_unlock` doc を「`bw unlock --raw` の stdout は `Stdio::null()` で破棄し成立確認は exit status のみ」へ修正し、FINDING 1 実装と一致させた（旧 doc の「stdout を成立確認に使う」記述を撤去）。

### 追加したテスト

- `support/protection/bw_login.rs` の `#[cfg(test)] mod tests`（default = gpg-backend build でのみ compile される real `bw` module 内）:
  - `reachability_check_discards_child_stdout`（FINDING 2）/ `login_and_unlock_discards_child_stdout`（FINDING 1）: `current_exe` 再起動による専用子プロセスで、PATH 先頭に置いた fake `bw`（一意 sentinel を自身の stdout へ出力）を実 process として起動し、対象関数を stdout 継承のまま実行。子プロセス stdout（= dotfiles が端末/JSON report へ出すストリーム）を pipe で捕捉し sentinel が現れないことを確認（`Stdio::null()` 破棄の実証）。marker file で fake `bw` の実起動も確認。process-global fd 退避や PATH 共有変更を使わず並行 `cargo test` でも安定。
  - `login_failure_is_stop_condition`: 非ゼロ終了 fake `bw` で停止条件（`Err`）かつ stdout 非漏洩を確認。
- `tests/secrets_cli.rs` の `verify_yubikey_runs_bw_login_external_check_ok`（FINDING 2 退行ガード）: `user_stdout()`（観測 sentinel 行を除いた dotfiles 本来の stdout）が単一の JSON document として parse 可能で、`checks` に `bw-login` を含むことを確認。reachability 前段で `bw` stdout を破棄しないと JSON が壊れる退行を捕捉する。

### remediation 確認結果（dev shell `direnv exec .` 内）

- `cargo build -p dotfiles-cli`（default = gpg-backend）: 成功。
- `cargo build -p dotfiles-cli --features secrets-internal-test-stub`: 成功。
- `cargo clippy -p dotfiles-cli --all-targets -- -D warnings`: 成功（警告 0）。
- `cargo clippy -p dotfiles-cli --all-targets --features secrets-internal-test-stub -- -D warnings`: 成功（警告 0）。
- `cargo fmt --check`: 成功（fmt 適用後 diff 0）。
- `cargo test -p dotfiles-cli --lib`: 209 passed / 0 failed（追加 4 件含む）。
- `cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli`: 42 passed / 0 failed。
- `cargo xtask check`: 全段階成功（fmt / check / clippy / test 209 unit / stub integration 42 / application route 64 / shell / workflows / nil / flake.lock / nix fmt / nix flake check = all checks passed）。
- 既存コマンド退行確認: restore-gpg / restore-pass / pass-remote / rotate-bws-token / verify-yubikey(bws/bw-login) の stub integration test が全て成功（退行なし）。
- 対象差分: 4 ファイル変更（`adapters/io/process.rs` / `ports/bw_login.rs` / `support/protection/bw_login.rs` / `tests/secrets_cli.rs`）。

### remediation 後の残課題

- 真のサービス到達確認（spec `## 停止条件` 節の `--check bw-login` 到達確認項）の実装は #16（実 `bw-login` 統合）の責務。#16 で `--check bw-login` を server 到達性確認へ広げ、port 契約 doc と本記録の既知制約を解消する。
- 実 `bw` CLI バイナリに対する end-to-end 検証（stdout 破棄の実 `bw` 経路含む）は #16 マージ時に確認する。本 remediation の stdout 破棄テストは fake `bw` による process 境界の検証にとどまる。

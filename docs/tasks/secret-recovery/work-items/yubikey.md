# #12 YubiKey 秘密情報保存

## 着手前要件: アーキテクチャレビューと計画

実装を開始する前に、以下を完了しなければならない。

1. **アーキテクチャレビュー**: 現行コード全体を対象に、V1〜V16 の各違反について「どの層から何を取り出してどこへ移設するか」を具体的に確認する。依存関係の連鎖（例: V8解消がV7の前提になる）を明確にする。
2. **リファクタリング計画の策定**: アーキテクチャレビューの結果を受けて、各ステップで行う変更の粒度・順序・完了条件を実装単位トラッカーに反映する。特に、複数の違反が同一ファイルに混在している場合の分割方針と新規ファイルの配置先を決定する。

アーキテクチャレビューと計画が完了してから「実装順序ガイド」および「固定実装単位トラッカー」の各ステップへ進む。

---

- 作業種別: `モジュール構造のゼロベース書き換えを含む規約適合リファクタリング`
- 作業目的: `dotfiles secrets yubikey*` と `verify-yubikey` を、現行の動作有無ではなくアーキテクチャ規約への厳密適合を基準に作り直す。責務境界が崩れている箇所を読み直し、モジュール分割、依存方向、入出力境界を再構成すること自体が仕事である。
- 構造完了条件:
  - `CLI` は clap option の型付けと公開 command 名だけを持つ。
  - `application` は use case の順序制御と外部境界呼び出しだけを持つ。
  - `domain` は YubiKey 実機、stdin/stdout、保護メモリ、外部 crate の I/O 型に依存しない。
  - `adapters` は実機 YubiKey と process I/O の接続に限定し、業務判断や use case 順序を持たない。
  - `adapters/` 配下に存在してよいファイルは「特定の port trait を実装するファイル」のみ。port trait を実装しないファイル（backend.rs・enrollment_json.rs・prompt.rs・stdin.rs・stdout.rs・terminal.rs・device_prompt.rs 等）は adapters/ から除去し、support/ 層（業務語彙を持たない場合）または port 実装ファイル内にインライン化すること。
  - `support` は保護メモリ、補助暗号、割り込み制御などの横断補助だけを持つ。
- 既存実装の流用方針: `既存コードは参照してよいが、責務境界が規約に合わない場合は大幅な再分割、再配置、削除を前提とする。`
- 規約違反の解消対象:

  この違反リストの判定基準は `docs/architecture/hexagonal-implementation-rules.md` の層ごとの責務と禁止事項に基づく。ファイル名は参考であり、判定は層への所属で行う。

  - **[層違反: application → adapter具体型への依存禁止]** **V1** `application.rs` が `adapters` / `adapters::input` を直接 import し、`DeviceBackend` / `RealSecretsBoundary` を直接組み立てている（`application` から `adapter` への依存禁止違反）。
  - **[層違反: application → concrete I/O・stdin・stdout policy は adapter 所有]** **V2** `application.rs` が `read_hidden_secret` / `read_visible_secret_line` / `read_protected_enrollment_secret_set` / `write_secret_to_stdout` を直接呼び、`println!` による report 出力を行っている（concrete I/O / stdin / stdout policy は `adapter` 所有規則違反）。
  - **[層違反: application → device handle は adapter 所有]** **V3** `application.rs` が `let mut device = ...` を全 use case で長寿命に保持し、`serial()` / `verify_pin()` / `check_management_auth_preconditions()` まで呼んでいる（device handle は `adapter` 所有規則違反）。
  - **[層違反: application配下 → adapter実装は adapters/ 層のみ許可]** **V4** `application/real_boundary.rs` が adapter 実装そのものを `application/` 配下に置いている（adapter と application の分離規則違反）。
  - **[層違反: application → concrete I/O・parser は adapter 所有]** **V5** `application/storage_service.rs` が永続書き込み、manifest の serde_json parse/serialize、blob decode、device precondition、summary 構築を一緒に持つ（concrete I/O / parser は `adapter` 所有規則違反）。
  - **[層違反: port → DTO・parser・prompt は adapter 所有]** **V6** `ports.rs` の `EnrollmentSecretSet` が port に DTO を置き、`SecretsBoundary` が `prompt_yes_no` / `stdin_is_terminal` / `stdout_is_terminal` / stdin JSON decode を含む（port への DTO 配置禁止・parser/prompt は `adapter` 所有規則違反）。
  - **[層違反: port → domain 以外への依存禁止]** **V7** `ports.rs` が `support::protection::{InterruptGuard, ProtectedSecret, SecretSession}` に依存している（port は domain にのみ依存可能規則違反）。
  - **[層違反: domain → port contract は port 層に置く]** **V8** `domain/model.rs` が `SecretDevice` trait を定義している（port contract は `port` に置く規則違反）。
  - **[層違反: domain → summary DTO は application 所有]** **V9** `domain/model.rs` が `CheckName` / `CheckStatus` / `EnrollSummary` / `VerifySummary` / `YubikeyRole` を保持している（summary / reporting DTO は `application` 所有規則違反）。
  - **[層違反: 層未確定 → 単一ファイル責務混在禁止]** **V10** `blob.rs` が層無所属のまま wire format / AEAD 暗号化 / content-key 生成 / port 呼び出し / ProtectedSecret 生成を同居させている（単一ファイル責務混在禁止違反）。
  - **[層違反: support → prompt・stdin・stdout policy は adapter 所有]** **V11** `support/terminal.rs` が TTY 判定 / prompt / raw mode / stdout 書き込みを `support` 配下に置いている（prompt/stdin/stdout policy は `adapter` 所有規則違反）。
  - **[層違反: adapters → port実装以外の公開禁止]** **V12** `adapters/input.rs` が hidden prompt / visible prompt / PIN input / stdin ingest / JSON decode / stdout terminal policy を 1 ファイルに集約し、port DTO に直接 decode している（port DTO 依存増幅・adapter 面混在違反）。
  - **[層違反: adapters → adapter 面は責務別に分割]** **V13** `adapters.rs` が backend selection / test-stub selection / interactive device selection / spare 交換 prompt を同一 surface に混在させている（adapter 面分割規則違反）。
  - **[層違反: tests → test double は tests 層所有・production export 禁止]** **V14** `secrets.rs` / `adapters/test_stub.rs` / `dotfiles-cli-secrets-test-contract` が production crate の command path に test double を feature-gate で埋め込んでいる（test double は tests 層所有・production export 禁止規則違反）。
  - **[層違反: tests → test double は tests 層所有・production export 禁止]** **V15** `application.rs` 内部 test module と `application/storage_service_tests.rs` が fake boundary / fake device を production tree 配下に置いている（同上）。
  - **[層違反: domain・port → I/O 型禁止]** **V16** `domain/model.rs` の `SecretDevice::write_unwrapped_key` が `std::io::Write` を domain/port 境界に持ち込んでいる（port / domain に I/O 型禁止規則違反）。
- 完了の判定条件（以下を全て満たすこと。1件でも残れば未完了とする）:
  - `application` が `adapter` の具体型を import しない（V1, V4 の解消）。
  - `application` が `println!` / stdin 読み取り / concrete device handle 操作を含まない（V2, V3 の解消）。
  - `ports` に DTO / parser / prompt が存在しない（V6 の解消）。
  - `ports` が `support` に依存しない（V7 の解消）。
  - `domain` に port contract / summary DTO / I/O 型が存在しない（V8, V9, V16 の解消）。
  - `support` に terminal I/O / prompt が存在しない（V11 の解消）。
  - `blob.rs` の責務が単一層に属する（V10 の解消）。
  - production コードに test double が含まれない（V14, V15 の解消）。
  - `adapters/` 配下に存在してよいファイルは「特定の port trait を実装するファイル」のみ。port trait を実装しないファイル（backend.rs・enrollment_json.rs・prompt.rs・stdin.rs・stdout.rs・terminal.rs・device_prompt.rs 等）は adapters/ から除去し、support/ 層（業務語彙を持たない場合）または port 実装ファイル内にインライン化すること（V12, V13 の解消）。
- レビュー合格条件: `上記完了の判定条件を全て確認し、アーキテクチャ規約に厳密に適合し、責務境界、依存方向、公開インターフェース境界に違反が残らないこと。動作するが構造が規約に合わないと判定される実装は合格としない。`

## 差戻し条件

以下のいずれか1件でも該当する場合、レビュー担当は `要修正` または `不合格` を返し、実装担当は当該ステップへ差戻しとなる。

- `application` が `adapter` の具体型を import している（V1, V4 未解消）
- `application` が `println!` / stdin 読み取り / concrete device handle 操作を含む（V2, V3 未解消）
- `ports` に DTO / parser / prompt が残存している（V6 未解消）
- `ports` が `support` に依存している（V7 未解消）
- `domain` に port contract / summary DTO / I/O 型が残存している（V8, V9, V16 未解消）
- `support` に terminal I/O / prompt が残存している（V11 未解消）
- `blob.rs` の責務が複数層にまたがっている（V10 未解消）
- production コードに test double が含まれている（V14, V15 未解消）
- `adapters/` 配下に port trait を実装しないファイル（backend.rs・enrollment_json.rs・prompt.rs・stdin.rs・stdout.rs・terminal.rs・device_prompt.rs 等）が存在している（V12, V13 未解消）
- 「動作する」という事実のみを根拠に完了報告している
- 粗粒度進捗注記: `#12` の design PR は `#21` として成立済みであり、現段階の主作業は implementation / code review / validation 面である。

## 現行レビュー差し戻しに基づく追加是正項目（2026-05-26）

2026-05-26 の規定レビュー担当一式によって、固定実装単位トラッカーの `完了` 扱いを維持できない残課題が再確認された。次回実装サイクルでは、以下を差戻し対象として明示的に解消すること。

- **レビュー/状態記録の扱い**
  - 今回の `コード差分なし` レビューは完了前進の根拠に使わず、`review-artifacts/yubikey/review.md` の不合格記録を差戻し正本として保持する。
  - 新たな実コード差分と確認証跡が揃うまで、固定実装単位トラッカーの `確認` / `レビュー` / `必要時の後続対応` は `未着手` のまま維持する。

- **ステップ3 を再開する（V6 再差戻し）**
  - `ports.rs` から DTO / prompt / stdin / stdout / terminal 判定に相当する契約を除去し、port を capability 宣言へ戻す。
- **ステップ4 を再開する（V10 再差戻し）**
  - `application/storage_service.rs` に再集中している blob / wire / 暗号 / manifest / 永続 I/O の責務を単一層へ再分離する。
- **ステップ5 を再開する（V12, V13 再差戻し + adapter seam 不整合解消）**
  - `ProcessSecretsBoundary` / `RealSecretDeviceFactory` を前提にする公開面と、`RealSecretsBoundary` を中核にする実装実体の不一致を解消し、adapter 境界契約を単一の seam に統一する。
  - `process_boundary.rs` から test double / test 用 backend 分岐を除去し、production adapter が実外部技術と port 契約の翻訳だけを担う状態へ戻す。
  - `adapters.rs` の公開面が port 実装以外に依存しないよう再設計し、`build_real_boundary` を含む adapter surface の責務を見直す。
- **ステップ6 を再開する（V5 再差戻し）**
  - `application/storage_service.rs` から manifest JSON parse/serialize、blob decode、永続 I/O、summary 構築の混在を除去する。
- **ステップ7 を再開する（V2, V3 再差戻し）**
  - `application.rs` から stdin 読み取り、stdout 書き込み方針、prompt、concrete device handle の長寿命保持と直接操作を除去する。
- **ステップ8 を再開する（V14 再差戻し + テスト実行基盤整合）**
  - production source tree から test double 責務を完全に除去し、`secrets-test-stub` 経路や `dotfiles-stub` 前提を tests 層 / test-support 側へ閉じる。
  - `Cargo.toml` と test 実行経路の定義を一致させ、`direnv exec . cargo check -p dotfiles-cli` と `direnv exec . cargo test -p dotfiles-cli --test secrets_cli --no-run` がレビュー前提として成立する状態へ戻す。
- **文書整合の是正**
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs` のモジュール説明コメントで現行実装と一致しない `real_boundary` 参照を修正する。

## 実装順序ガイド（推奨）

規約違反 V1〜V16 の解消は、依存関係の順序を考慮して以下の順で着手することを推奨する。

1. **V8, V16 を先に解消する**（domain → port の依存整理）
   - V8：`domain/model.rs` の `SecretDevice` を `ports.rs` へ移設
   - V16：`SecretDevice::write_unwrapped_key` の `std::io::Write` を除去
2. **V9 を解消する**（domain の summary DTO 除去）
   - domain が clean になった後で summary DTO を application 層へ移設
3. **V6, V7 を解消する**（port の DTO・parser・prompt・support 依存を除去）
   - V6：port contract を最小 capability 契約に縮小（`EnrollmentSecretSet` DTO 除去、`prompt_yes_no`/`stdin_is_terminal`/`stdout_is_terminal` を adapter 所有へ）
   - V7：V6 の整理に伴い `InterruptGuard`/`ProtectedSecret`/`SecretSession` をシグネチャから除去し `support::protection` 依存を断つ
4. **V10 を解消する**（blob.rs の責務分割）
   - wire format・AEAD・port 呼び出しを各層へ分離
5. **V11, V12, V13 を解消する**（adapter 面の整理）
   - terminal I/O・prompt を support から adapter へ移設
   - adapter 面を個別 adapter に分割
6. **V4, V5 を解消する**（application 配下の adapter 実装を移設）
7. **V1, V2, V3 を解消する**（application の concrete I/O 依存を除去）
8. **V14, V15 を解消する**（test double を production tree から除去）

差戻し時は本ガイドの該当ステップへ戻る。

## 実装セッション制約

1 回の実装セッションで実施する範囲は、固定実装単位トラッカーの最初の `未着手` ステップのみとする。以下をすべて順に実行し、**git commit が成功するまでセッションを終了してはならない**。

1. 固定実装単位トラッカーを確認し、最初の `未着手` エントリを特定する
2. そのステップに対応する違反のみを解消する
3. `direnv exec . cargo check -p dotfiles-cli` を実行し、エラーゼロを確認する
4. トラッカー（`docs/tasks/secret-recovery/tasks.md` および本ファイル）のそのエントリを `完了` に更新する
5. **以下のコマンドを必ず実行してコミットを完了させる（省略・省エネは禁止）**:
   ```
   git add <変更した全ファイル>
   git commit -m "refactor(secrets): #12 ステップN でVX,VY を解消"
   ```
   `git commit` コマンドを実際に実行し、`[feat/yubikey-secret-storage <hash>]` の出力を確認すること。
6. `git log --oneline -1` でコミットが記録されたことを確認する
7. 次のステップには進まず停止する

## 違反ファイルマップ（実装担当参照用）

作業定義の `規約違反の解消対象` V1〜V16 と対象ファイルの対応を示す。

| 違反 | 対象ファイル | 解消操作の方向 |
|------|------------|--------------|
| V1, V3 | `src/secrets/application.rs` | adapter import を除去。device handle は adapter 所有へ移設。 |
| V2 | `src/secrets/application.rs` | `println!` / stdin 読み取りを adapter へ移設。port 経由に統一。 |
| V4 | `src/secrets/application/real_boundary.rs` | adapters 層へ移設する。 |
| V5 | `src/secrets/application/storage_service.rs` | serde_json parse / blob decode を adapter へ移設。 |
| V6 | `src/secrets/ports.rs` | `EnrollmentSecretSet` DTO を除去。`SecretsBoundary` を最小 capability 契約に分割。 |
| V7 | `src/secrets/ports.rs` | `support::protection` への直接依存を除去する。V6（`SecretsBoundary` 整理）と連動しており、V6 で `InterruptGuard`/`ProtectedSecret`/`SecretSession` をシグネチャから除去するか domain 層へ移設することで解消する。ステップ1では V8/V16 のみ対象とし V7 はステップ3（V6 と同時）で解消する。 |
| V8 | `src/secrets/domain/model.rs` | `SecretDevice` を ports 層へ移設。 |
| V9 | `src/secrets/domain/model.rs` | summary DTO（`EnrollSummary` 等）を application 層へ移設。 |
| V10 | `src/secrets/blob.rs` | 層ごとに分割し、wire format は domain/wire、AEAD は support/crypto 相当、port 呼び出しは adapter へ。 |
| V11 | `src/secrets/support/terminal.rs` | adapters/terminal（仮称）へ移設し support からは除去。 |
| V12 | `src/secrets/adapters/input.rs` | prompt / stdin / JSON decode / stdout policy を個別 adapter に分割。DTO への直接 decode を廃止。 |
| V13 | `src/secrets/adapters.rs` | backend selection / test-stub selection / device prompt を各専用 adapter に分離。 |
| V14, V15 | `src/secrets/adapters/test_stub.rs`、`src/secrets/application/storage_service_tests.rs`、`dotfiles-cli-secrets-test-contract` | production feature path から除去し tests/ 層へ移設。 |
| V16 | `src/secrets/domain/model.rs` | `write_unwrapped_key` の `impl Write` 引数をバイト列 / protected 型へ変更し I/O 型を除去。 |

## 固定実装単位トラッカー

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 実装 ステップ1: V8,V16（domain SecretDevice→ports移設・io::Write除去） | 完了 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ2: V9（domain summary DTO除去） | 完了 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ3: V6,V7（port DTO/parser/prompt除去・support依存除去） | 完了 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ4: V10（blob.rs責務分割） | 未着手 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ5: V11,V12,V13（adapter面整理） | 未着手 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ6: V4,V5（application配下adapter移設） | 未着手 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ7: V1,V2,V3（application concrete I/O依存除去） | 未着手 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ8: V14,V15（test double除去） | 未着手 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 確認 | 未着手 | `review-artifacts/yubikey/confirmation.md` | [implementation-guidelines.md#確認](../../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 未着手 | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#レビュー](../../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 未着手 | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#必要時の後続対応](../../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

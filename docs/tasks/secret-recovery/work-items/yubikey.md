# #12 YubiKey 秘密情報保存

## 着手前要件: アーキテクチャレビューと計画

実装を開始する前に、以下を完了しなければならない。

1. **アーキテクチャレビュー**: 現行コード全体を対象に、V1〜V16 の各違反について「どの層から何を取り出してどこへ移設するか」を具体的に確認する。依存関係の連鎖（例: V8解消がV7の前提になる）を明確にする。
2. **リファクタリング計画の策定**: アーキテクチャレビューの結果を受けて、各ステップで行う変更の粒度・順序・完了条件を実装単位トラッカーに反映する。特に、複数の違反が同一ファイルに混在している場合の分割方針と新規ファイルの配置先を決定する。

アーキテクチャレビューと計画が完了してから「実装順序ガイド」および「固定実装単位トラッカー」の各ステップへ進む。

---

- 作業種別: `モジュール構造のゼロベース書き換えを含む規約適合リファクタリング`
- 現行サイクル状態: `再レビュー待ち`
- 作業目的: `dotfiles secrets yubikey*` と `verify-yubikey` を、現行の動作有無ではなくアーキテクチャ規約への厳密適合を基準に作り直す。責務境界が崩れている箇所を読み直し、モジュール分割、依存方向、入出力境界を再構成すること自体が仕事である。
- 構造完了条件:
  - `CLI` は clap option の型付けと公開 command 名だけを持つ。
  - `application` は use case の順序制御と外部境界呼び出しだけを持ち、各 use case は `rust/dotfiles-cli/src/secrets/application/` 直下の sibling file にある単一の `run_*` 関数として表現される。
  - `application.rs` は sibling `run_*.rs` 群の module 配線だけを持ち、`application/use_case/` ディレクトリを新設せず、`mod.rs` と `#[path = "..."]` を使わない。
  - `application` は `domain` と `port` にのみ依存し、乱数生成は port 経由でのみ取得する。use case 独自型を定義せず、use case が扱う型は `domain` 層定義に限定する。
  - use case-to-use case call を禁止し、use case 層での logic commonization を行わない。重複が必要なら許容し、共通責務は下位層へ押し戻す。
  - `domain` は YubiKey 実機、stdin/stdout、保護メモリ、外部 crate の I/O 型に依存しない。
  - secret 本文の許可表現は `ProtectedSecret` のみとし、`Vec<u8>` や `String` を domain/application/ports で secret 値保持に使わない。
  - `adapters` は実機 YubiKey と process I/O の接続に限定し、業務判断や use case 順序を持たない。
  - `adapters/` 配下に存在してよいファイルは「特定の port trait を実装するファイル」のみ。port trait を実装しないファイル（backend.rs・enrollment_json.rs・prompt.rs・stdin.rs・stdout.rs・terminal.rs・device_prompt.rs 等）は adapters/ から除去し、support/ 層（業務語彙を持たない場合）または port 実装ファイル内にインライン化すること。
  - `support` は保護メモリ、補助暗号、割り込み制御などの横断補助を主に担う。`rust/dotfiles-cli/src/secrets/support/process_io.rs` のような process-generic な標準入出力補助は残してよいが、YubiKey-specific な prompt 方針、use case 手順、device 選択判断を `support` に持ち込んではならない。
- 既存実装の流用方針: `既存コードは参照してよいが、責務境界が規約に合わない場合は大幅な再分割、再配置、削除を前提とする。`
- 規約違反の解消対象:

  この違反リストの判定基準は `docs/architecture/hexagonal-implementation-rules.md` の層ごとの責務と禁止事項に基づく。ファイル名は参考であり、判定は層への所属で行う。

  - **[層違反: application → adapter具体型への依存禁止]** **V1** `application.rs` が `adapters` / `adapters::input` を直接 import し、`DeviceBackend` / `RealSecretsBoundary` を直接組み立てている（`application` から `adapter` への依存禁止違反）。
  - **[層違反: application → concrete I/O・stdin・stdout policy は adapter 所有]** **V2** `application.rs` が `read_hidden_secret` / `read_visible_secret_line` / `read_protected_enrollment_secret_set` / `write_secret_to_stdout` を直接呼び、`println!` による report 出力を行っている（concrete I/O / stdin / stdout policy は `adapter` 所有規則違反）。
  - **[層違反: application → device handle は adapter 所有]** **V3** `application.rs` が `let mut device = ...` を全 use case で長寿命に保持し、`serial()` / `verify_pin()` / `check_management_auth_preconditions()` まで呼んでいる（device handle は `adapter` 所有規則違反）。
  - **[層違反: application配下 → adapter実装は adapters/ 層のみ許可]** **V4** `application/` 配下に adapter 実装責務（実 I/O・実機 discovery・外部 API 変換）が混入している（adapter と application の分離規則違反）。
  - **[層違反: application → concrete I/O・parser・protected-secret ownership は adapter/support 所有]** **V5** `application/run_*.rs` に永続書き込み、manifest の serde_json parse/serialize、blob decode、device precondition、summary 構築、AEAD 呼び出し順序、`SecretSession` / `ProtectedSecret` 系の所有が混在している（concrete I/O / parser / crypto 実装詳細 / protected-secret ownership は `application` 保有禁止違反）。
  - **[層違反: port → DTO・parser・prompt は adapter 所有]** **V6** `ports.rs` の `EnrollmentSecretSet` が port に DTO を置き、`SecretsBoundary` が `prompt_yes_no` / `stdin_is_terminal` / `stdout_is_terminal` / stdin JSON decode を含む（port への DTO 配置禁止・parser/prompt は `adapter` 所有規則違反）。
  - **[層違反: port → domain 以外への依存禁止]** **V7** `ports.rs` が `support::protection::{InterruptGuard, ProtectedSecret, SecretSession}` に依存している（port は domain にのみ依存可能規則違反）。
  - **[層違反: domain → port contract は port 層に置く]** **V8** `domain/model.rs` が `SecretDevice` trait を定義している（port contract は `port` に置く規則違反）。
  - **[層違反: application → use case 独自型禁止・domain 型限定]** **V9** `CheckName` / `CheckStatus` / `EnrollSummary` / `VerifySummary` / `YubikeyRole` のような use case outcome 型を `application` 側へ所有・移設してはならない。use case が扱う型は `domain` 層定義に限定し、application 独自型の導入を禁止する。
  - **[層違反: application → ユースケース単位分割 + 技術詳細排除]** **V10** `application.rs` が sibling `run_*.rs` の module 配線を超える責務を持っている、または `application/run_*.rs` が wire format / AEAD / protected-secret ownership / device 操作 / use case 手順以外の責務を再混在させている。各 use case は sibling file に 1 つの `run_*` 関数として表現し、`application.rs` は module 配線専用に限定する必要がある（1 use case = 1 function 原則違反・application 層責務混在違反）。
  - **[層違反: support → feature-specific terminal policy / orchestration は禁止]** **V11** support 層に YubiKey-specific な prompt 方針、device 選択判断、use case 手順、feature 固有の stdout policy が混入している。`rust/dotfiles-cli/src/secrets/support/process_io.rs` の残存自体は defect ではなく、判定は「process-generic な補助か」「feature 固有の判断を持ち込んでいるか」で行う。
  - **[層違反: adapters → port実装以外の公開禁止]** **V12** `adapters/piv_io.rs` が input modality（prompt/stdin/stdin-json）を port 契約へ逆流させる公開面を維持し、report DTO 変換まで一体化している（port 契約汚染・adapter 面混在違反）。
  - **[層違反: adapters → adapter 面は責務別に分割]** **V13** `adapters.rs` / `adapters/piv_io.rs` が device selection・interactive prompt・stdin JSON decode・report 出力変換を同一 seam で保持し、差し替え単位が不明確になっている（adapter 面分割規則違反）。
  - **[経路違反: production command path の境界維持]** **V14** `adapters/piv_io.rs` 内の device selection 配線は same-route 原則を満たす単一路でなければならない。禁止対象は、利用者向けの別 CLI / 別 binary、および `--test-stub-yubikey`・`yubikey_runtime`・`secrets-test-stub` feature / env などを使って command-scenario を分岐し production behavior を変形すること、または production command path 自体を差し替える port-boundary swap である。feature / env の存在自体を blanket 禁止してはならない。一方で、同一 production command path と同一 port 契約を維持したまま行う test 時の dependency selection / fixture selection（例: テストハーネス側での許可済み実装選択、固定 fixture 入力）は許可し、V14 違反として扱ってはならない。
  - **[配置・責務評価違反: production adapter 面の整合]** **V15** production adapter 面に test stub / fake device / fixture state を持ち込まない。PIV/YubiKey 実機 adapter は port 契約と外部技術の翻訳だけを持ち、test fixture は production command path を変形しない検証層へ分離する。
  - **[層違反: domain・port → I/O 型禁止]** **V16** `domain/model.rs` の `SecretDevice::write_unwrapped_key` が `std::io::Write` を domain/port 境界に持ち込んでいる（port / domain に I/O 型禁止規則違反）。
- 完了の判定条件（以下を全て満たすこと。1件でも残れば未完了とする）:
  - `application` が `adapter` の具体型を import しない（V1, V4 の解消）。
  - `application` が `println!` / stdin 読み取り / concrete device handle 操作を含まない（V2, V3 の解消）。
  - `application` は `domain` と `port` にのみ依存し、乱数生成を port 経由で取得する。`application.rs` は sibling `run_*.rs` の module 配線だけを持ち、production use case entrypoint としての各 sibling `run_*.rs` は単一の `run_*` 関数だけを持ち、use case-to-use case call や use case 層での logic commonization を持たない（V1, V5, V9, V10 の解消）。この制約は `#[cfg(test)] mod tests` および `*_tests.rs` には適用しない。
  - `ports` に DTO / parser / prompt が存在しない（V6 の解消）。
  - `ports` が `support` に依存しない（V7 の解消）。
  - `domain` に port contract / I/O 型が存在せず、use case outcome 型は `domain` 側にのみ定義され presentation 仕様を含まない（V8, V9, V16 の解消）。
  - `support` に feature-specific な terminal I/O / prompt / orchestration が存在しない。`rust/dotfiles-cli/src/secrets/support/process_io.rs` のような process-generic 補助は許容するが、YubiKey-specific 文言、device 選択判断、use case 手順を含めない（V11 の解消）。
  - `application/run_*.rs`・`adapters/piv_io.rs`・`domain/wire.rs` 間で、wire/crypto/device/use-case の責務境界が単一層原則に従って分離されている（V10 の解消）。
  - `SelectedDeviceAdapter` は same-route 原則を満たす単一 production command path を維持し、`--test-stub-yubikey` / `yubikey_runtime` / `secrets-test-stub` feature / env / 別 binary / 別 CLI / command-scenario branching / production command path を差し替える port-boundary swap を、product behavior 変形として導入しない（V14 の解消）。
  - same-route 判定では、同一 production command path と同一 port 契約を維持した test 時の dependency selection / fixture selection を禁止しない。これらは command path 変形や product behavior 変更を伴わない限り許可される。
  - production adapter 面に test stub / fake device / fixture state を残さない（V15 の解消）。
  - `adapters/` 配下に存在してよいファイルは「特定の port trait を実装するファイル」のみ。port trait を実装しないファイル（backend.rs・enrollment_json.rs・prompt.rs・stdin.rs・stdout.rs・terminal.rs・device_prompt.rs 等）は adapters/ から除去し、support/ 層（業務語彙を持たない場合）または port 実装ファイル内にインライン化すること（V12, V13 の解消）。
- レビュー合格条件: `上記完了の判定条件を全て確認し、アーキテクチャ規約に厳密に適合し、責務境界、依存方向、公開インターフェース境界に違反が残らないこと。動作するが構造が規約に合わないと判定される実装は合格としない。`
  - `application/` と `adapters/` の private helper について、helper 単位で責務が層責務に一致することを説明できること。説明できない helper が1件でもあれば設計誤りとして不合格。
  - helper 増殖が見られる場合、review では helper の個別修正で閉じず、port capability 設計の粗さ（契約粒度不足）が原因でないかを必ず判定すること。原因が port 設計にある場合は port 再分割まで要求すること。
  - file-level 分割の実施有無だけで合格にしてはならない。helper ごとの責務確定が確認できるまで V10/V12/V13 は未解消扱いとする。

## 差戻し条件

以下のいずれか1件でも該当する場合、レビュー担当は `要修正` または `不合格` を返し、実装担当は当該ステップへ差戻しとなる。

- `application` が `adapter` の具体型を import している（V1, V4 未解消）
- `application` が `println!` / stdin 読み取り / concrete device handle 操作を含む（V2, V3 未解消）
- `ports` に DTO / parser / prompt が残存している（V6 未解消）
- `ports` が `support` に依存している（V7 未解消）
- `domain` に port contract / I/O 型が残存している、または use case outcome 型が `application` 側へ移されている（V8, V9, V16 未解消）
- `support` に feature-specific な terminal I/O / prompt / orchestration が残存している。`rust/dotfiles-cli/src/secrets/support/process_io.rs` の残存だけを理由に defect 扱いしている場合も、V11 の判定誤りとして差戻し対象に含める。
- `application.rs` が sibling `run_*.rs` の module 配線を超える責務を持っている、`application/use_case/` ディレクトリを導入している、または production use case entrypoint としての sibling `run_*.rs` ごとの単一 `run_*` 関数原則を破っている（V10 未解消）。`#[cfg(test)] mod tests` と `*_tests.rs` はこの違反判定対象に含めない。
- use case-to-use case call または use case 層での logic commonization を導入している
- same-route を崩す command path 分岐（`secrets-test-stub` feature / env、`--test-stub-yubikey`、`yubikey_runtime`、別 binary / 別 CLI / command-scenario branching / production command path を差し替える port-boundary swap を使い、product behavior を変形する分岐）が導入されている（V14 未解消）
- 同一 production command path と同一 port 契約を維持した test 時の dependency selection / fixture selection まで禁止・不合格判定している（V14 判定誤り）
- production adapter 面に test stub / fake device / fixture state が残存している（V15 未解消）
- `adapters/` 配下に port trait を実装しないファイル（backend.rs・enrollment_json.rs・prompt.rs・stdin.rs・stdout.rs・terminal.rs・device_prompt.rs 等）が存在している（V12, V13 未解消）
- `application/` または `adapters/` の private helper について責務説明ができず、helper 単位の責務判定を省略している
- helper 増殖を port 契約粒度の問題として評価せず、file-level 分割のみで解消扱いにしている
- `application/run_*.rs` の公開 entrypoint または core workflow 非自明 helper に doc comment coverage が不足し、停止条件・責務境界・caller responsibility・why の説明が欠落している
- `ports`/`adapters`/`support` の層責務境界を担う非自明要素で doc comment が欠落し、責任分界説明がない
- secret 本文を `ProtectedSecret` 以外（`Vec<u8>` / `String` / 独自平文 wrapper）で保持・受け渡ししている
- 「動作する」という事実のみを根拠に完了報告している
- 粗粒度進捗注記: `#12` の design PR は `#21` として成立済みであり、現段階の主作業は implementation / code review / validation 面である。

## 現行レビュー差し戻しに基づく追加是正項目（2026-05-26）

### 2026-05-26 追加実装サイクル結果

- 解消済み: 未解決 1,2,3,4,5,6,7,9
- 未解消継続: 未解決 10（最終集約判定の再更新）
- 証跡同期: 未解決 10 は `review.md` / `confirmation.md` / 本ファイルへ同内容追記で同期済み

2026-05-26 の規定レビュー担当一式によって、固定実装単位トラッカーの `完了` 扱いを維持できない残課題が再確認された。次回実装サイクルでは、以下を差戻し対象として明示的に解消すること。

- **レビュー/状態記録の扱い**
  - 今回の `コード差分なし` レビューは完了前進の根拠に使わず、`review-artifacts/yubikey/review.md` の不合格記録を差戻し正本として保持する。
  - 新たな実コード差分と確認証跡が揃うまで、固定実装単位トラッカーの `確認` / `レビュー` / `必要時の後続対応` は `未着手` のまま維持する。

- **ステップ3 を再開する（V6 再差戻し）**
  - `ports.rs` から DTO / prompt / stdin / stdout / terminal 判定に相当する契約を除去し、port を capability 宣言へ戻す。
  - 2026-05-26 実装サイクル追記: `SecretsBoundary` から `require_serial` / `require_option` / `read_enrollment_json_bytes` / `ask_continue_rotation` を除去し、enrollment 入力は field 単位 capability へ再設計した。`ports.rs` の `support` 依存は引き続き存在しない（V7 維持）。
- **ステップ4 を再開する（V10 再差戻し）**
  - `application.rs` を sibling `run_*.rs` の module 配線専用に保ち、各 use case を `application/` 直下の sibling `run_*.rs` にある単一 `run_*` 関数として維持する。
  - `application/use_case/` ディレクトリ、`mod.rs`、`#[path = "..."]` を導入しない。
  - 各 use case から blob / wire / 暗号 / manifest / 永続 I/O / protected-secret ownership を除去し、use case-to-use case call と logic commonization を持ち込まない。
- **ステップ5 を再開する（V12, V13 再差戻し + adapter seam 不整合解消）**
  - `adapters.rs` の公開面と `adapters/piv_io/` 配下の実装責務を一致させ、adapter 境界契約を単一の seam に統一する。
  - `adapters/piv_io/` の分岐は same-route 原則に合わせ、command-scenario branching や port-boundary swap を持ち込まない形で整理する。
  - `adapters.rs` の公開面が port 実装以外に依存しないよう再設計し、adapter surface の責務を見直す。
- **ステップ6 を再開する（V5 再差戻し）**
  - `application/run_*.rs` の helper 群から manifest JSON parse/serialize、blob decode、永続 I/O、summary 構築、AEAD 呼び出し順序、`SecretSession` / `ProtectedSecret` 所有の混在を除去する。
  - use case 独自型の導入で責務を逃がさず、use case が扱う型を domain 層定義だけに戻す。
- **ステップ7 を再開する（V2, V3 再差戻し）**
  - `application.rs` から stdin 読み取り、stdout 書き込み方針、prompt、concrete device handle の長寿命保持と直接操作を除去する。
- **ステップ8 を再開する（V14,V15 再差戻し + 経路/責務整合）**
  - `adapters/piv_io.rs` 内の `SelectedDeviceAdapter` は same-route を維持し、`secrets-test-stub` feature / env 分岐・`--test-stub-yubikey`・`yubikey_runtime`・別 CLI/別 binary などを使った product behavior 変形としての command path 変形を解消する。
  - same-route 判定では、同一 production command path と同一 port 契約を維持した test 時の dependency selection / fixture selection を許可対象として扱う。禁止対象は product behavior を切り替える command-scenario branching と production command path 差し替えに限定する。
  - production adapter 面に test stub / fake device / fixture state を残さず、テストは production command path を feature/env で差し替えない範囲へ限定する。
  - secret 本文は `ProtectedSecret` 型以外で扱わない前提を維持する。
  - `rust/dotfiles-cli-secrets-test-stub/` を復活させない。
  - `Cargo.toml` と test 実行経路の定義を一致させ、`direnv exec . cargo check -p dotfiles-cli` と `direnv exec . cargo test -p dotfiles-cli --test secrets_cli --no-run` がレビュー前提として成立する状態へ戻す。
- **文書整合の是正**
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` と `rust/dotfiles-cli/src/secrets/adapters/piv_io/` のモジュール説明コメントを現行実装の責務境界と一致させる。
  - `rust/dotfiles-cli/src/secrets/application/run_*.rs` の公開 use-case entrypoint と非自明 helper、ならびに sibling `run_*.rs` を配線する `application.rs` に必要な doc comment coverage を付与し、`what` だけでなく `why` を明記する。
  - `application/run_*.rs` に限らず、`ports`/`adapters`/`support` の層境界説明が必要な非自明 type/function で doc comment 欠落を残さない。欠落はレビュー blocker として扱う。

## 実装順序ガイド（推奨）

規約違反 V1〜V16 の解消は、依存関係の順序を考慮して以下の順で着手することを推奨する。

1. **V8, V16 を先に解消する**（domain → port の依存整理）
   - V8：`domain/model.rs` の `SecretDevice` を `ports.rs` へ移設
   - V16：`SecretDevice::write_unwrapped_key` の `std::io::Write` を除去
2. **V9 を解消する**（use case outcome 型の domain 統一）
   - use case 独自型を禁止し、use case outcome 型を domain 層定義へ統一する。application 側への移設は解消ではなく再違反である。
3. **V6, V7 を解消する**（port の DTO・parser・prompt・support 依存を除去）
   - V6：port contract を最小 capability 契約に縮小（`EnrollmentSecretSet` DTO 除去、`prompt_yes_no`/`stdin_is_terminal`/`stdout_is_terminal` を adapter 所有へ）
   - V7：V6 の整理に伴い `InterruptGuard`/`ProtectedSecret`/`SecretSession` をシグネチャから除去し `support::protection` 依存を断つ
4. **V10 を解消する**（`application.rs` / `application/run_*.rs` / `adapters/piv_io.rs` / `domain/wire.rs` 間の責務再分離）
   - wire format・AEAD・port 呼び出しを各層へ分離
5. **V11, V12, V13 を解消する**（adapter 面の整理）
   - terminal I/O・prompt を support から adapter へ移設
   - adapter 面を個別 adapter に分割
6. **V4, V5 を解消する**（application 配下の adapter 実装を移設）
7. **V1, V2, V3 を解消する**（application の concrete I/O 依存を除去）
8. **V14, V15 を解消する**（same-route 維持 + PIV/YubiKey 固有 concrete 実装の配置/責務整合）

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
   git commit -m "<type>(secrets): <#12 の保存点を表す簡潔な説明>"
   ```
   `git commit` コマンドを実際に実行し、`[feat/yubikey-secret-storage <hash>]` の出力を確認すること。メッセージは実施した保存点の内容に合わせ、過去の固定例へ無理に合わせない。
6. `git log --oneline -1` でコミットが記録されたことを確認する
7. 次のステップには進まず停止する

## 違反ファイルマップ（実装担当参照用）

作業定義の `規約違反の解消対象` V1〜V16 と対象ファイルの対応を示す。

| 違反 | 対象ファイル | 解消操作の方向 |
|------|------------|--------------|
| V1, V2, V3 | `src/secrets/application.rs` | adapter import を除去し、`println!` / stdin 読み取り / device handle 直接操作を adapter 所有へ移設して port 経由に統一する。 |
| V4 | `src/secrets/application/` | application 配下の adapter 実装責務を `adapters/` へ移設する。 |
| V5 | `src/secrets/application/run_*.rs` | serde_json parse / blob decode / AEAD 呼び出し順序 / `SecretSession`・`ProtectedSecret` 所有を application から除去し、adapter・support・domain の責務境界へ戻す。 |
| V6 | `src/secrets/ports.rs` | `EnrollmentSecretSet` DTO を除去。`SecretsBoundary` を最小 capability 契約に分割。 |
| V7 | `src/secrets/ports.rs` | `support::protection` への直接依存を除去する。V6（`SecretsBoundary` 整理）と連動しており、V6 で `InterruptGuard`/`ProtectedSecret`/`SecretSession` をシグネチャから除去するか domain 層へ移設することで解消する。ステップ1では V8/V16 のみ対象とし V7 はステップ3（V6 と同時）で解消する。 |
| V8 | `src/secrets/domain/model.rs` | `SecretDevice` を ports 層へ移設。 |
| V9 | `src/secrets/domain/model.rs`、`src/secrets/application/summary.rs`（存在する場合） | use case outcome 型（`EnrollSummary` 等）を domain 層へ統一し、application 独自型を残さない。 |
| V10 | `src/secrets/application.rs`、`src/secrets/application/run_*.rs`、`src/secrets/adapters/piv_io.rs`、`src/secrets/domain/wire.rs` | `application.rs` を sibling `run_*.rs` の module 配線専用に限定し、production use case entrypoint としての `application/` 直下 sibling `run_*.rs` ごとに単一 `run_*` 関数を維持する。use-case 手順、device 外部 API 変換、wire parser/serializer、protected-secret ownership の責務を層ごとに再分離し、use case-to-use case call と use case 層 commonization を禁止する。`#[cfg(test)] mod tests` と `*_tests.rs` はこの単一 `run_*` 制約の対象外。 |
| V11 | `src/secrets/support/` | feature-specific な terminal I/O / prompt / orchestration を support から除去する。`support/process_io.rs` のような process-generic 補助は残してよい。 |
| V12 | `src/secrets/adapters/piv_io.rs`、`src/secrets/ports.rs` | input modality を port 契約から除去し、adapter 側で実装詳細として閉じる。 |
| V13 | `src/secrets/adapters.rs`、`src/secrets/adapters/piv_io.rs` | device selection / input / report の責務境界を明示し、adapter seam を分離する。 |
| V14, V15 | `src/secrets/adapters/piv_io.rs`、`rust/dotfiles-cli/tests/secrets_cli.rs` | same-route を崩す分岐を解消し、production adapter 面から test stub / fake device / fixture state を除去する。 |
| V16 | `src/secrets/domain/model.rs` | `write_unwrapped_key` の `impl Write` 引数をバイト列 / protected 型へ変更し I/O 型を除去。 |

## 固定実装単位トラッカー

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 実装 ステップ1: V8,V16（domain SecretDevice→ports移設・io::Write除去） | 完了 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ2: V9（use case outcome 型の domain 統一） | 完了 | 実コード差分 | [#実装順序ガイド推奨](#実装順序ガイド推奨) |
| 実装 ステップ3: V6,V7（port DTO/parser/prompt除去・support依存除去） | 完了 | 実コード差分 | [#現行レビュー差し戻しに基づく追加是正項目2026-05-26](#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ4: V10（責務再分離） | 完了 | 実コード差分 | [#現行レビュー差し戻しに基づく追加是正項目2026-05-26](#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ5: V11,V12,V13（adapter面整理） | 完了 | 実コード差分 | [#現行レビュー差し戻しに基づく追加是正項目2026-05-26](#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ6: V4,V5（application配下adapter移設） | 完了 | 実コード差分 | [#現行レビュー差し戻しに基づく追加是正項目2026-05-26](#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ7: V1,V2,V3（application concrete I/O依存除去） | 完了 | 実コード差分 | [#現行レビュー差し戻しに基づく追加是正項目2026-05-26](#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 実装 ステップ8: V14,V15（same-route維持 + stub配置/責務整合） | 完了 | 実コード差分 | [#現行レビュー差し戻しに基づく追加是正項目2026-05-26](#現行レビュー差し戻しに基づく追加是正項目2026-05-26) |
| 確認 | 再レビュー待ち | `review-artifacts/yubikey/confirmation.md` | [implementation-guidelines.md#確認](../../../secret-recovery/implementation-guidelines.md#確認) |
| レビュー | 再レビュー待ち | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#レビュー](../../../secret-recovery/implementation-guidelines.md#レビュー) |
| 必要時の後続対応 | 修正済み | `review-artifacts/yubikey/review.md` | [implementation-guidelines.md#必要時の後続対応](../../../secret-recovery/implementation-guidelines.md#必要時の後続対応) |

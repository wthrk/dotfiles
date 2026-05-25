# 運用整合レビュー記録 — YubiKey 秘密情報保存（2026-05-25）

- レビュー担当: 運用整合レビュー担当
- 対象作業項目: `docs/tasks/secret-recovery/work-items/yubikey.md`
- レビュー日: 2026-05-25
- レビュー対象: リポジトリ現行コード全体（差分ではなく実ファイルを直接精査）
- 本記録は独立した新規セッションによる判定であり、先行する同名記録の内容を引き継がない。

---

判定: 要修正

判定要約: 集約後レビュー判定が未記入のまま「完了」とされており、ゲート条件の監査可能性が不成立。加えて `adapters/` 配下に作業定義差戻し条件が禁止する `pub(crate)` 非port関数が残存し、`dotfiles-stub` バイナリが未定義のため統合テストがコンパイル不可。

根拠:

- **所見 1: ゲート条件の証跡欠落（監査可能性不成立）**: `review.md` の集約後レビュー判定フィールドがテンプレートプレースホルダ `<合格|要修正|不合格>` のままであり、構造・仕様適合・参照整合レビュー担当の判定フィールドも `未記入` のまま。`workflow.md` §6「コミット着手ゲート」は「必要レビュー役割の結果が記録され、集約後レビュー判定が合格」を必須条件とするが、この証跡が存在しない状態で `tasks.md` に「状態: 完了」が記録されている。強制可能性・監査可能性に具体的懸念がある状態で合格とできない。
- **所見 2: `adapters/` 配下の `pub(crate)` 非port関数（差戻し条件直接抵触）**: `adapters/terminal.rs`（8関数）、`adapters/prompt.rs`（3関数）、`adapters/stdin.rs`（1関数）、`adapters/stdout.rs`（1関数）、`adapters/backend.rs`（enum・constructor）、`adapters/yubikey.rs`（struct 2件・factory 2件）が port trait 実装でないにも関わらず `pub(crate)` で公開されている。作業定義差戻し条件「`adapters/` 配下のファイルで port trait 実装以外の関数・型・定数が `pub(crate)` 以上の可視性で外部公開されている（V12, V13 未解消）」に直接抵触する。
- **所見 3: 統合テストがコンパイル不可（完了条件の検証が強制不能）**: `tests/secrets_cli.rs` が `env!("CARGO_BIN_EXE_dotfiles-stub")` を参照するが `rust/dotfiles-cli/Cargo.toml` に `[[bin]] name = "dotfiles-stub"` が定義されていない。`[features]` セクション自体が存在しないため `secrets-test-stub` feature も未定義。CLI コマンドをスタブ backend で動作検証する全統合テストがコンパイルエラーで実行不能であり、完了条件の実行可能な検証が存在しない。

---

## 1. 直接確認した事実

### 1-1. ゲート条件の証跡欠落

`docs/tasks/secret-recovery/review-artifacts/yubikey/review.md` を直接読んで確認した結果:

- `構造レビュー担当` 判定: `<合格|要修正|不合格>`（テンプレートプレースホルダ）、根拠: `未記入`
- `運用整合レビュー担当` 判定: `<合格|要修正|不合格>`（テンプレートプレースホルダ）、根拠: `未記入`
- `セキュリティレビュー担当` 判定: `合格`（記入済み、詳細は `review-security-2026-05-25.md` 参照）
- `仕様適合レビュー担当` 判定: `<合格|要修正|不合格>`（テンプレートプレースホルダ）、根拠: `未記入`
- `参照整合レビュー担当` 判定: `<合格|要修正|不合格>`（テンプレートプレースホルダ）、根拠: `未記入`
- `集約後レビュー判定`: `<合格|要修正|不合格>`（テンプレートプレースホルダ）

`docs/tasks/tasks.md` YubiKey 項目: 「状態: `完了`」
`docs/tasks/secret-recovery/tasks.md` YubiKey 項目: 「状態: `完了`」
`docs/tasks/secret-recovery/work-items/yubikey.md` 固定実装単位トラッカー: 全行「完了」（「レビュー」「必要時の後続対応」を含む）

`implementation-review-judgement.md` 集約規則: 「必須担当が全員 `合格` の場合のみ `集約後レビュー判定: 合格`」および「コミット連動規則: コミット関連作業は `集約後レビュー判定` の記録後に開始できる」。

4役割中3役割が未判定のまま、かつ集約後レビュー判定がプレースホルダのまま「完了」が記録されている。ゲート条件の監査可能性は不成立。

### 1-2. `adapters/` 配下の `pub(crate)` 非port関数

各ファイルを直接読んで確認した結果、port trait 実装ではない `pub(crate)` シンボルが以下のとおり存在する。

**`adapters/terminal.rs`**:
- `pub(crate) fn stdin_is_terminal() -> bool`（行 24）— TTY 判定 helper
- `pub(crate) fn stdout_is_terminal() -> bool`（行 29）— TTY 判定 helper
- `pub(crate) fn prompt_yes_no(...)` （行 36）— yes/no prompt helper
- `pub(crate) fn wait_for_enter(...)`（行 53）— Enter 待機 helper
- `pub(crate) fn write_all_stdout(...)`（行 66）— stdout 書き込み helper
- `pub(crate) fn read_hidden_input(...)`（行 75）— hidden input helper
- `pub(crate) fn read_terminal_line_interruptible(...)`（行 121）— terminal 読み取り helper
- `pub(crate) fn read_terminal_line_until(...)`（行 161）— terminal 読み取り helper

**`adapters/prompt.rs`**:
- `pub(crate) fn read_visible_line_bytes(...)`（行 27）— visible prompt helper
- `pub(crate) fn read_hidden_bytes(...)`（行 40）— hidden prompt helper
- `pub(crate) fn read_yubikey_pin_raw()`（行 49）— PIN 読み取り helper

**`adapters/stdin.rs`**（直接確認済み）:
- `pub(crate) fn read_stdin_bytes(...)` — stdin 読み取り helper

**`adapters/stdout.rs`**:
- `pub(crate) fn write_secret_to_stdout(...)`（行 22）— stdout 書き込み helper

**`adapters/backend.rs`**:
- `pub(crate) enum DeviceBackend`（行 11）— backend 選択 enum
- `pub(crate) fn from_test_flag(...)`（行 18）— backend constructor

**`adapters/yubikey.rs`**:
- `pub(crate) struct YubikeyInteraction<'a>`（行 44）— adapter 内部 I/O 境界型（フィールドも `pub(crate)`）
- `pub(crate) struct YubikeySelectionCandidate<'a>`（行 49）— device 選択候補型（フィールドも `pub(crate)`）
- `pub(crate) fn open_device(...)`（行 74）— device factory 関数
- `pub(crate) fn open_spare_device(...)`（行 142）— spare device factory 関数

`adapters.rs` では `pub(super) mod terminal` および `pub(super) mod input` として再エクスポートされており、`adapters` 親 module の `secrets` から `adapters::terminal::*` へのパスが到達可能。

作業定義差戻し条件の文言:

> `adapters/` 配下のファイルで port trait 実装以外の関数・型・定数が `pub(crate)` 以上の可視性で外部公開されている（V12, V13 未解消）

上記シンボル群はこの条件に直接抵触する。`pub(crate)` は crate 全体への公開であり、「port trait を実装する型・メソッド以外が外部公開されていない」という完了条件を満たさない。

### 1-3. 統合テストのコンパイル不可

`rust/dotfiles-cli/Cargo.toml` を直接読んで確認した結果:

```toml
[[bin]]
name = "dotfiles"
path = "src/main.rs"
```

`[[bin]] name = "dotfiles-stub"` は定義されていない。`[features]` セクション自体が存在しない。

`tests/secrets_cli.rs` を直接読んで確認した結果（行 924、972、1028、1104）:

```rust
Command::new(env!("CARGO_BIN_EXE_dotfiles-stub"))
CommandBuilder::new(env!("CARGO_BIN_EXE_dotfiles-stub"))
env!("CARGO_BIN_EXE_dotfiles-stub")
env!("CARGO_BIN_EXE_dotfiles-stub")
```

`CARGO_BIN_EXE_<name>` は Cargo が `[[bin]]` セクションで定義したバイナリのパスに展開されるマクロである。`dotfiles-stub` が未定義のためコンパイル時にマクロ展開が失敗する。

`dotfiles-cli-secrets-test-contract` crate（`Cargo.toml` 確認済み）は library として存在し、contract 定数を定義しているが、stub バイナリの実体（binary entrypoint）が存在しない。`tests/secrets_cli.rs` が依存する `dotfiles-stub` binary の `--test-stub-yubikey` フラグも、`adapters/backend.rs`（`DeviceBackend::from_test_flag` が常に `Real` を返す）では処理できない。

setup / put / get / enroll-primary / enroll-spare / rotate-bws-token / verify-yubikey の各 CLI コマンドをスタブ backend で検証する統合テスト全体がコンパイルエラーで実行不能。完了条件の実行可能な検証が存在しない状態である。

---

## 2. 運用整合観点での確認（問題なし）

直接コードを読んで確認した結果、以下の項目は運用整合の観点で問題を確認できなかった。

**非対話実行時のゲート強制**
`require_serial`・`require_option`・`require_stdin_pipe`・`require_stdin_json_pipe`・`require_stdout_pipe` がすべて `RealSecretsBoundary` で実装されており、device open より前に実行される順序が application 層のコードで固定されている。エラーメッセージは具体的な option 名を含み、CI/CD ログからの診断が可能。

**interrupt 処理の強制**
`InterruptGuard` が `open_spare_device` と `prompt_continue_rotation` で使われており、Ctrl-C 割り込み時の安全な停止が構造として固定されている。

**報告出力の一貫性**
enroll / rotate / verify 系 use case で `boundary.write_report()` が実行される。途中失敗時は partial summary を先に出力してから error を返す設計になっており、再実行支援情報が残る。

**V14/V15（test double の production tree 混入禁止）**
`application.rs` の `FakeBoundary`・`FakeDevice` は `#[cfg(test)]` ブロック内に限定されており、production build では除外される。`application/storage_service.rs` の `FakeDevice` も同様。production コードへの test double 混入は確認できなかった（ただし統合テストがコンパイルできないため実行レベルでの確認は不可）。

**V1〜V11, V16（application の concrete I/O 依存除去）**
`application.rs` が `adapters` 具体型を import しない（V1/V4）、`println!`/stdin 直接読み取りがない（V2/V3）、`ports.rs` に DTO/parser/prompt がない（V6）、`ports.rs` が `support` に依存しない（V7）、`domain/model.rs` に port contract/summary DTO/I/O 型がない（V8/V9/V16）、`support/terminal.rs` が空ファイル（移設済み、V11）、wire format が `domain/wire.rs` に分離済み（V10）を現行コードを直接読んで確認した。

---

## 3. 差戻し事項

### 差戻し事項 A（必須）: 統合テストのコンパイル可能化

`rust/dotfiles-cli/Cargo.toml` に `dotfiles-stub` バイナリ（または等価な stub 実行パス）を定義し、`tests/secrets_cli.rs` がコンパイルできる状態にすること。stub backend への切り替え機構（`--test-stub-yubikey` フラグの処理）を production 実行から分離した形で実装すること。

`cargo test -p dotfiles-cli --test secrets_cli` が実行完了し、テストが通過することを確認記録に残すこと。

### 差戻し事項 B（必須）: V12/V13 差戻し条件の解消

`adapters/` 配下の port trait 実装ではない `pub(crate)` シンボルを `pub(super)` 以下の可視性に変更すること。対象は所見 1-2 に列挙した全シンボル（`terminal.rs`・`prompt.rs`・`stdin.rs`・`stdout.rs`・`backend.rs`・`yubikey.rs` 各ファイルのシンボル）。

変更後に `cargo check` でコンパイルエラーがないこと、および差戻し条件「port trait 実装以外の関数・型・定数が `pub(crate)` 以上の可視性で外部公開されていない」を実コード追跡で確認し、確認記録に残すこと。

### 差戻し事項 C（必須）: 全必須レビュー役割の判定記録と集約

構造レビュー担当・仕様適合レビュー担当・参照整合レビュー担当の各判定を `review.md` へ記録し、集約後レビュー判定を確定すること。全員合格が確認された後でのみ集約後レビュー判定を `合格` として記録できる。

### 差戻し事項 D（付随）: 完了記録の是正

差戻し事項 A〜C およびその他のレビュー担当の差戻し事項を解消し、全必須レビュー担当が `合格` を返した後に `docs/tasks/tasks.md`・`docs/tasks/secret-recovery/tasks.md`・`work-items/yubikey.md` の「完了」記録を正当化する証跡を揃えること。

---

## 4. 確認したファイル（直接精査）

- `docs/tasks/tasks.md`
- `docs/tasks/secret-recovery/tasks.md`
- `docs/tasks/secret-recovery/work-items/yubikey.md`
- `docs/tasks/secret-recovery/review-artifacts/yubikey/review.md`
- `docs/task-governance/workflow.md`
- `docs/task-governance/implementation-review-judgement.md`
- `rust/dotfiles-cli/Cargo.toml`
- `rust/dotfiles-cli/src/secrets.rs`
- `rust/dotfiles-cli/src/secrets/application.rs`
- `rust/dotfiles-cli/src/secrets/application/storage_service.rs`
- `rust/dotfiles-cli/src/secrets/ports.rs`
- `rust/dotfiles-cli/src/secrets/adapters.rs`
- `rust/dotfiles-cli/src/secrets/adapters/real_boundary.rs`
- `rust/dotfiles-cli/src/secrets/adapters/backend.rs`
- `rust/dotfiles-cli/src/secrets/adapters/terminal.rs`
- `rust/dotfiles-cli/src/secrets/adapters/input.rs`
- `rust/dotfiles-cli/src/secrets/adapters/prompt.rs`
- `rust/dotfiles-cli/src/secrets/adapters/stdin.rs`
- `rust/dotfiles-cli/src/secrets/adapters/stdout.rs`
- `rust/dotfiles-cli/src/secrets/adapters/stdout.rs`
- `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
- `rust/dotfiles-cli/src/secrets/domain/model.rs`
- `rust/dotfiles-cli/src/secrets/domain/wire.rs`
- `rust/dotfiles-cli/src/secrets/support/terminal.rs`
- `rust/dotfiles-cli/tests/secrets_cli.rs`
- `rust/dotfiles-cli-secrets-test-contract/Cargo.toml`
- `rust/dotfiles-cli-secrets-test-contract/src/lib.rs`
- `rust/Cargo.toml` （workspace ルート）

# 構造レビュー — YubiKey タスク現行コード全体評価（2026-05-25 再実施）

- レビュー日時: 2026-05-25（再実施）
- レビュー担当: 構造レビュー担当
- 対象コードパス: `rust/dotfiles-cli/src/`
- レビュー種別: 現行コード全体を独立新規セッションとして評価
- 目的: YubiKey タスクが「完了」状態とされることが妥当かどうかの判定

> **注**: 本レビューは直前のレビューサイクルを引き継がない独立した新規セッションである。過去のレビュー記録・実装担当の報告を代替にせず、現行コードを直接精査した。

---

## ステップ 1 — 哲学的検証（コードを読む前に問いを確定し、コード精査後に回答）

`docs/architecture/review-checklist.md` の各層「レビュー時の問い」をすべて先に確認したうえで、現行コードを読んで回答する。

---

### adapters/ 層

#### 問い 1: このコードは「翻訳」のみをしているか。外部技術の型をポートの型に変換する以外の判断・順序制御・ポリシー決定を持ち込んでいないか。

回答: 各 adapter ファイルの責務は外部技術とポート契約の翻訳に概ね限定されている。`real_boundary.rs` は `SecretsBoundary` を実装し翻訳者の役割を果たす。`yubikey.rs` は `SecretDevice` port を実機 YubiKey PIV へ接続する。しかし「翻訳のみ」という哲学の評価以前に、次の問いで公開面の哲学的違反が確認された。

#### 問い 2: `pub(crate)` または `pub(super)` で公開されているシンボルについて — port の契約を果たすためか。「内部で使いやすいから」「呼び出し元が必要としているから」は公開の正当化にならない。

回答: **哲学的違反を確認。**

以下のシンボルが port trait 実装でないにもかかわらず `pub(crate)` で公開されている。`pub(crate)` はクレート全体から到達可能な公開面であり、「外部公開ではない」という意味にはならない。現時点で adapter 外から参照されていない事実は、将来の依存を招く構造的結合点の存在を否定しない。

**`adapters/terminal.rs`（8 関数すべて）:**
- `pub(crate) fn stdin_is_terminal()` — TTY 判定 helper。port trait 実装ではない。
- `pub(crate) fn stdout_is_terminal()` — TTY 判定 helper。port trait 実装ではない。
- `pub(crate) fn prompt_yes_no(...)` — prompt helper。port trait 実装ではない。
- `pub(crate) fn wait_for_enter(...)` — terminal 待機 helper。port trait 実装ではない。
- `pub(crate) fn write_all_stdout(...)` — stdout 書き込み helper。port trait 実装ではない。
- `pub(crate) fn read_hidden_input(...)` — hidden input helper。port trait 実装ではない。
- `pub(crate) fn read_terminal_line_interruptible(...)` — terminal 読み取り helper。port trait 実装ではない。
- `pub(crate) fn read_terminal_line_until(...)` — terminal 読み取り helper（deadline 付き）。port trait 実装ではない。

**`adapters/prompt.rs`（3 関数）:**
- `pub(crate) fn read_visible_line_bytes(...)` — port trait 実装ではない。
- `pub(crate) fn read_hidden_bytes(...)` — port trait 実装ではない。
- `pub(crate) fn read_yubikey_pin_raw()` — port trait 実装ではない。

**`adapters/stdin.rs`（1 関数）:**
- `pub(crate) fn read_stdin_bytes(...)` — port trait 実装ではない。

**`adapters/stdout.rs`（1 関数）:**
- `pub(crate) fn write_secret_to_stdout(...)` — port trait 実装ではない。

**`adapters/yubikey.rs`（一部）:**
- `pub(crate) struct YubikeyInteraction<'a>` — adapter 内部 I/O 接続型。port trait 実装ではない。
- `pub(crate) struct YubikeySelectionCandidate<'a>` — adapter 内部 helper 型。port trait 実装ではない。
- `pub(crate) fn open_device(...)` — adapter 内部 factory 関数。port trait 実装ではない。
- `pub(crate) fn open_spare_device(...)` — adapter 内部 factory 関数。port trait 実装ではない。
- （`pub(crate) struct YubikeySecretDevice` は `SecretDevice` trait を実装するため合格）

**`adapters/backend.rs`（2 シンボル）:**
- `pub(crate) enum DeviceBackend` — adapter 内部 backend 選択 enum。port trait 実装ではない。
- `pub(crate) fn from_test_flag(...)` — adapter 内部 constructor。port trait 実装ではない。

合計: **port trait 実装でない `pub(crate)` シンボルが 19 件存在する。**

`hexagonal-implementation-rules.md` の「公開面最小化は構造的制約である」「意図していない結合がコンパイルを通過して蓄積し、後の変更コストとして顕在化する」に対する哲学的違反が確認された。

#### 問い 3: このコードが存在する理由を一文で言えるか。「外部技術Xとポート契約Yの間を翻訳するため」と言えるか。

回答: 個々のファイルの存在理由は言える。しかし `pub(crate)` 公開により、adapter 内部の実装 helper を adapter 境界の外に見せている状態は「翻訳者」役割を超えた公開面であることが確認された。

#### 問い 4: このファイルを削除して別の技術に差し替えたとき、application/ や domain/ のコードを変更する必要が生じるか。

回答: 現時点では `application/` が `adapters::` のシンボルを直接 import していないため、直接的な依存は生じない。しかし `pub(crate)` 公開が存在する限り、将来そのような依存が形成されることを構造として許容しており、これが哲学的違反の本質である。

**adapters/ 層の哲学的判定: 違反あり（公開面最小化の哲学的違反）**

---

### application/ 層

#### 問い 1: このコードはユースケースの「順序」を知っているだけか。具体的なデバイス、I/O、パーサーの実装を知っていないか。

回答: `application.rs` および `application/storage_service.rs` は `SecretsBoundary` / `SecretDevice` trait を介してのみ外部境界を操作する。`println!` の直接呼び出し・stdin 読み取りは存在しない。`storage_service.rs` での `use std::io::Write` は `ProtectedInputBuffer`（support 層の保護メモリ型）への書き込みのためであり、terminal I/O ではない。違反なし。

#### 問い 2: boundary の呼び出しはポートの契約を通じているか。アダプターの具体型を直接知っているか。

回答: すべての境界操作は `SecretsBoundary` および `SecretDevice` trait を通じている。adapter 具体型の import なし。違反なし。

#### 問い 3: application/ 配下にアダプター実装が紛れ込んでいないか。

回答: `application/` 配下に adapter 実装ファイルは存在しない。違反なし。

#### 問い 4: CLI→Web API に移植したとき変更は最小限か。

回答: `SecretsBoundary` の差し替えで対応可能。application のコードはインターフェース固有の知識を持っていない。違反なし。

**application/ 層の哲学的判定: 違反なし**

---

### domain/ 層

#### 問い 1: このコードは「ビジネスルール」だけを知っているか。技術選定を知っていないか。

回答: `domain/model.rs` は PIV object ID、secret 名、manifest、blob の型付けを定義する。`serde` による JSON シリアライズは manifest の wire format として domain に属する。`nom` による wire format parser は `domain/wire.rs` に分離されている。外部 SDK 型・端末状態・プロセス状態への依存なし。違反なし。

#### 問い 2: 型を差し替えたとき変更が必要か。

回答: `PivObjectId`、`SecretName`、`SecretManifest`、`SecretBlob` はこのドメインの核心概念のモデリングであり、別技術への差し替えは domain 自体の変更を意味する。違反なし。

**domain/ 層の哲学的判定: 違反なし**

---

### ports/ 層

#### 問い 1: trait またはこの型は「ドメインが何を必要とするかの宣言」になっているか。実装詳細を含んでいないか。

回答: `SecretsBoundary` は capability 契約の trait であり、TTY 判定・prompt 文言・入力形式の詳細を含まない。`read_yubikey_pin_bytes` というメソッド名は "YubiKey" という技術固有名を含むが、このシステムの本質的な依存対象が YubiKey であり、trait の意図宣言として解釈できる。

#### 問い 2: struct/enum は技術の詳細か、ドメインの意図か。

回答: `EnrollmentBytes` は raw bytes の受け渡し専用 struct であり parser・prompt を含まない。`ports.rs` は `use zeroize::Zeroizing` により外部 crate 型へ依存しているが、`Zeroizing` はメモリ安全プリミティブとして許容される範囲と判断する。

**ports/ 層の哲学的判定: 違反なし（懸念事項として `zeroize` 依存を記録するが不合格の根拠とはしない）**

---

### support/ 層

#### 問い 1: このコードに業務語彙が含まれていないか。機能固有の名前が現れていたら support に置くべきではない。

回答: **哲学的違反を確認。**

`support/protection.rs` の `InterruptGuard::run_yubikey_operation` メソッドは "yubikey" という特定ハードウェアベンダーの製品名をメソッド名に含む。`review-checklist.md` の support 層規則「機能固有 vocabulary・command 名・role 名を含まないこと」に違反する。

`support/terminal.rs` は `// moved to adapters/terminal.rs` の 1 行のみであり問題なし。

#### 問い 4: このコードを別のまったく異なるプロダクトにそのままコピーして使えるか。

回答: `InterruptGuard::run_yubikey_operation` という名前は使えない。YubiKey とは無関係のプロダクトにこのメソッド名は意味をなさない。実装内容（前後で interrupt flag を確認する汎用ラッパー）は汎用だが、名前が業務語彙を持っている。

**support/ 層の哲学的判定: 違反あり（`InterruptGuard::run_yubikey_operation` に業務語彙混入）**

---

## ステップ 1 総合判定

以下の哲学的違反が確認された:

1. **adapters/ 層**: port trait 実装でない `pub(crate)` シンボルが 19 件存在する（公開面最小化の哲学的違反）
2. **support/ 層**: `InterruptGuard::run_yubikey_operation` に特定製品名 "yubikey" が含まれる（業務語彙禁止の哲学的違反）

**ステップ 1 で哲学違反が 2 種確認されたため、ステップ 2 に進まず `判定: 不合格` を確定する。**

---

## ステップ 2 — チェックリスト照合（参考記録のみ）

ステップ 1 で哲学違反を確認したため正式スキップ。adapters/ の公開シンボル全列挙（ステップ 1 の根拠として実施済み）の結果は上記に記録した。

---

## 判定

判定: 不合格

判定要約: adapters/ 層で port trait 実装でない `pub(crate)` シンボルが 19 件存在し、公開面最小化の哲学的違反が確認された。また support/ 層の `InterruptGuard::run_yubikey_operation` に業務語彙（YubiKey 製品名）が混入しており業務語彙禁止の哲学的違反が確認された。

根拠:

- **adapters/ 公開面規則違反（主要・哲学的違反）**: `rust/dotfiles-cli/src/secrets/adapters/` 配下の `terminal.rs`（8 件）、`prompt.rs`（3 件）、`stdin.rs`（1 件）、`stdout.rs`（1 件）、`yubikey.rs`（4 件）、`backend.rs`（2 件）において、port trait を実装する型またはそのメソッド実装でないシンボルが `pub(crate)` で公開されている。合計 19 件。`pub(crate)` はクレート全体から到達可能な公開面であり、「adapter 内部で使いやすいから」または「呼び出し元が必要としているから」という理由は `hexagonal-implementation-rules.md` が明示的に禁止している正当化理由に当たる。現時点で adapter 外からの参照が存在しないという事実はこの構造的違反を解消しない。

- **support/ 業務語彙混入違反（哲学的違反）**: `rust/dotfiles-cli/src/secrets/support/protection.rs` の `InterruptGuard::run_yubikey_operation` メソッド名に "yubikey" という特定ハードウェアベンダーの製品名が含まれる。実装内容（interrupt flag を前後で確認する汎用ラッパー）は機能中立だが、メソッド名が業務語彙を持つことは `review-checklist.md` の「機能固有 vocabulary・command 名・role 名を含まないこと」に違反する。別プロダクトへのコピー利用が名前の意味上不可能なことがこれを裏付ける。

- **適合確認**: application/ 層は adapter 具体型を直接 import せず、port trait 経由のみで操作している。domain/ 層は外部 SDK 型・端末状態・プロセス状態に依存していない。test double は `#[cfg(test)]` ブロック内に閉じており production tree への混入はない。これらは適合している。

## 是正要求事項

### 必須（差戻し要因）

**[A] adapters/ 公開面の最小化（19 件）**

以下の変更が必要。各シンボルは `adapters` module 内部でのみ使用されているため、`pub(crate)` → `pub(super)` への変更で adapter 内部の相互参照を維持できる。

- `adapters/terminal.rs`: 全 8 関数を `pub(crate)` → `pub(super)` または private へ変更
- `adapters/prompt.rs`: 全 3 関数を `pub(crate)` → `pub(super)` または private へ変更
- `adapters/stdin.rs`: `read_stdin_bytes` を `pub(crate)` → `pub(super)` または private へ変更
- `adapters/stdout.rs`: `write_secret_to_stdout` を `pub(crate)` → `pub(super)` または private へ変更
- `adapters/yubikey.rs`: `YubikeyInteraction`（フィールド含む）、`YubikeySelectionCandidate`（フィールド含む）、`open_device`、`open_spare_device` を `pub(crate)` → `pub(super)` または private へ変更
- `adapters/backend.rs`: `DeviceBackend`（enum）、`from_test_flag` を `pub(crate)` → `pub(super)` または private へ変更

変更後、`adapters.rs`（親モジュール）が `pub(super) fn build_real_boundary()` のみを親（`secrets`）へ公開する形に集約されること。

**[B] support/ 業務語彙の除去**

- `rust/dotfiles-cli/src/secrets/support/protection.rs` の `InterruptGuard::run_yubikey_operation` を業務語彙を持たない名前（例: `run_operation`、`run_interruptible`、`run_checked` 等）に改名する。
- `SecretSession` の同名メソッドも同様に改名する（`SecretSession::run_yubikey_operation` が `protection.rs` 内で委譲している）。
- 改名に伴い、`application.rs` および `storage_service.rs` の呼び出し箇所も更新する。

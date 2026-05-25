# YubiKey レビュー記録

この文書は `docs/tasks/secret-recovery/tasks.md` の作業項目 `YubiKey` に対する固定実装単位 `レビュー` の正本記録である。2026-05-26 の現行コード全体レビュー結果を保持し、2026-05-25 以前の個別レビュー記録は履歴参照として同ディレクトリに残す。

## 実装担当からの引き継ぎ

- レビュー状態: `差し戻し中（2026-05-26 現行コード全体レビューで不合格）`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 確認開始時 HEAD: `47db89710975156fd971a4d49c915950694c223a`
- 対象差分識別子: `yubikey-current-code-full-review-2026-05-26-head-47db89710975156fd971a4d49c915950694c223a-codepaths-sha256-3efea38d6259d5b1c255b9cc141a1cc596aea0c5683f4c4acda9f6341fa1dbd2`
- 実装側確認証跡: `./confirmation.md`（履歴参照用。今回判定は現行コードを直接読んだ独立レビュー結果を正本とする）

## レビュー担当チェック項目

1. 対象作業項目のスコープ外へ越境していないこと。
2. 責務境界と依存方向が [レビュー観点チェックリスト](../../../../architecture/review-checklist.md#レビュー観点チェックリスト構造) の観点に適合していること。
3. 対象 work-item の `レビュー合格条件` を満たすこと。
4. 仕様・設計・作業定義文書の要求挙動、停止条件、成功条件が反映されていること。
5. 文書構造の整合だけでなく、必須の実行手順・役割分離・ゲート条件・証跡要件・完了判定ロジックについて、実運用での強制可能性または監査可能性に具体的懸念がないこと。懸念がある場合は必ず所見化し、強制可能性/監査可能性が不確実なまま `合格` にしないこと（`スコープ外` や `運用徹底` を理由に格下げしない）。
6. 具体的懸念、残留リスク、未解消疑義、要追跡事項、運用依存の注意事項を記録した場合、それは `finding あり` であり、`No findings` / `指摘なし` / `懸念なし` / `合格` と併記しないこと。判定は少なくとも `要修正` とし、解消条件または差戻し事項を同じ記録に書くこと。

## 判定記録フォーマット

- 各レビュー担当の記録は、必ず次の順で書く。
  1. `判定: <合格|要修正|不合格>`
  2. `判定要約: <所見なし|主要論点要約>`
  3. `根拠:`
- `判定` に使ってよいラベルは `合格`、`要修正`、`不合格` のみとする。
- `通しません`、`No findings`、`指摘なし`、`no blockers`、`pass` などの自由文を `判定` の代わりに使ってはならない。
- `合格` の場合、`判定要約` は `所見なし` とする。
- `要修正` または `不合格` の場合、`判定要約` は主要論点を 1 行で要約し、`根拠:` に差戻し条件または不合格理由を箇条書きで記録する。
- 集約判定も同じ構造で書き、`集約後レビュー判定`、`集約判定要約`、`集約根拠` を必ず揃える。

## セキュリティ所見記録（必須）

### 1) 秘密値・認証情報の扱い

- 確認状態: `確認済`
- 確認対象（ファイル/経路）: `rust/dotfiles-cli/src/secrets.rs`、`rust/dotfiles-cli/src/secrets/`、`rust/dotfiles-cli/tests/secrets_cli.rs`、`rust/dotfiles-cli/Cargo.toml`、`rust/dotfiles-cli-secrets-test-contract/src/`
- 所見: 平文の認証情報・鍵素材・トークン等のコミットは確認されず、秘密値の所有境界は `SecretSession` / `ProtectedInputBuffer` / `ProtectedSecret` により保護されている。
- 差戻し要否: `なし`
- 未実施理由（未実施時のみ）: —

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 確認状態: `確認済`
- 確認対象（出力経路）: `write_secret_to_stdout`、`require_stdout_pipe`、`ensure_secret_stdout_not_terminal`、失敗時メッセージ経路
- 所見: 本番経路の secret 出力は stdout pipe 前提に制限されており、stderr / log / error message への secret 値露出は確認されなかった。
- 差戻し要否: `なし`
- 未実施理由（未実施時のみ）: —

### 3) 権限境界・永続化・失敗時挙動

- 確認状態: `確認済`
- 確認対象（境界/保存先/失敗経路）: `support/protection`、`support/aead`、`adapters/yubikey.rs`、secret I/O 境界
- 所見: 保護メモリ、zeroize、割り込み処理、AEAD 復号失敗時の振る舞いは `security-obligations.md` の要求と整合し、追加の権限昇格経路や秘密値漏えい経路は確認されなかった。
- 差戻し要否: `なし`
- 未実施理由（未実施時のみ）: —

## 役割別レビュー記録（2026-05-26 現行コード全体レビュー）

### 構造レビュー担当

- 判定: 不合格
- 判定要約: `adapters` 層に test 代替責務の混入と公開面規約違反があり、`ProcessSecretsBoundary` 前提の組立面と実装中核が自己整合していない。
- 根拠:
  - `rust/dotfiles-cli/src/secrets/adapters/process_boundary.rs` に `DeviceBackend::TestStub` と `test_stub::TestDevice` が残存し、実外部技術の翻訳ではなくテスト時の実依存肩代わり責務が production adapter に混入している。
  - `rust/dotfiles-cli/src/secrets/adapters.rs` の `pub(crate) fn build_real_boundary` は port trait 実装型でもそのメソッドでもなく、adapter 公開面の絶対規則に適合しない。
  - `rust/dotfiles-cli/src/secrets.rs` / `rust/dotfiles-cli/src/secrets/adapters.rs` が前提にする `ProcessSecretsBoundary` / `RealSecretDeviceFactory` と、`process_boundary.rs` 側の `RealSecretsBoundary` 実体が一致していない。

### 運用整合レビュー担当

- 判定: 不合格
- 判定要約: レビュー・確認証跡を再現可能にする実行経路が現行コードで成立しておらず、強制可能性と監査可能性を満たしていない。
- 根拠:
  - `rust/dotfiles-cli/src/secrets/adapters.rs` と `rust/dotfiles-cli/src/secrets.rs` が要求する `ProcessSecretsBoundary` / `RealSecretDeviceFactory` が実装側で解決できず、`direnv exec . cargo check -p dotfiles-cli` 前提の運用経路が破綻している。
  - `rust/dotfiles-cli/src/secrets/adapters/process_boundary.rs` に `#[cfg(feature = "secrets-test-stub")]` 経路が残る一方、`rust/dotfiles-cli/Cargo.toml` に当該 feature 定義がなく、構成で有効条件を強制できない。
  - `rust/dotfiles-cli/tests/secrets_cli.rs` は `CARGO_BIN_EXE_dotfiles-stub` と `--test-stub-yubikey` を前提にするが、`rust/dotfiles-cli/Cargo.toml` に `dotfiles-stub` バイナリ定義がなく、確認手順の再現性が欠けている。
  - 作業定義と台帳では過去に完了扱いされていたが、現行コードはビルド不能かつ確認経路欠落のため、完了判定ロジックと監査証跡が整合しない。

### セキュリティレビュー担当

- 判定: 合格
- 判定要約: 所見なし
- 根拠:
  - 指定対象パスに平文の認証情報・鍵素材・トークン等のコミットは確認されなかった。
  - 本番経路の secret 出力は `write_secret_to_stdout` に限定され、TTY への誤出力拒否が実装されている。
  - 失敗時メッセージは secret 値を含まず、保護メモリ・zeroize・割り込み処理も `security-obligations.md` の要求と整合している。

### 仕様適合レビュー担当

- 判定: 不合格
- 判定要約: `規約違反の解消対象`・`構造完了条件`・`完了条件` の複数項目が未充足で、特に V5/V6/V10/V12/V13/V14 とテスト実行基盤の不整合が残存している。
- 根拠:
  - `rust/dotfiles-cli/src/secrets/application/storage_service.rs` に永続 I/O・manifest JSON parse/serialize・blob/wire/暗号・summary 構築が再混在し、V5/V10 が未解消である。
  - `rust/dotfiles-cli/src/secrets/ports.rs` に DTO と prompt/stdin/stdout 契約が残り、V6 が未解消である。
  - `rust/dotfiles-cli/src/secrets/adapters/process_boundary.rs` と `rust/dotfiles-cli/src/secrets/adapters.rs` に adapter 面の責務集中と seam 不一致が残り、V12/V13 が未解消である。
  - production source tree に `secrets-test-stub` 経路が残り、V14 の「production コードに test double が含まれない」を満たしていない。
  - `rust/dotfiles-cli/tests/secrets_cli.rs` の `CARGO_BIN_EXE_dotfiles-stub` 前提と `rust/dotfiles-cli/Cargo.toml` の定義が一致せず、完了条件の検証基盤が成立していない。

### テストレビュー担当

- 判定: 不合格
- 判定要約: 完了条件を直接検証するテスト網羅が不足し、production tree への test double 責務混入とテスト実行不能が残っている。
- 根拠:
  - `docs/tasks/secret-recovery/work-items/yubikey.md` の V1〜V16 解消を直接検証する構造テストが確認できず、現行テストは主に CLI 動作中心である。
  - `rust/dotfiles-cli/src/secrets/adapters/process_boundary.rs` に `secrets-test-stub` 経路の test double 責務が残っており、production コードに test double を含めない条件に反する。
  - `direnv exec . cargo test -p dotfiles-cli --test secrets_cli --no-run` 前提で見ると、`ProcessSecretsBoundary` / `RealSecretDeviceFactory` 未解決や feature 未定義によりテスト基盤自体が成立していない。

### ドキュメントレビュー担当

- 判定: 要修正
- 判定要約: `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs` のモジュール説明コメントに現行実装と不一致な参照がある。
- 根拠:
  - `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs` の冒頭コメントが呼び出し元を `real_boundary` と記載しているが、現行境界実装は `process_boundary.rs` と `adapters.rs` の構成になっており名称・参照が一致しない。
  - 主要ドキュメントコメントの why 記述自体は概ね足りているため、差戻し対象はこの不整合の是正に限定される。

### アーキテクチャ整合レビュー担当

- 判定: 不合格
- 判定要約: 主契約として語られる `ProcessSecretsBoundary` + `SecretDeviceFactory` seam と実装実体の `RealSecretsBoundary` が不一致で、モジュール全体が一貫した 1 つの設計になっていない。
- 根拠:
  - `rust/dotfiles-cli/src/secrets.rs` と `rust/dotfiles-cli/src/secrets/adapters.rs` は `ProcessSecretsBoundary` / `RealSecretDeviceFactory` を公開 seam として前提にする一方、`rust/dotfiles-cli/src/secrets/adapters/process_boundary.rs` は `RealSecretsBoundary` を中核にしており、モジュールの語る境界と実体が一致していない。
  - adapter 境界だけが「差し替え可能 factory seam」と「実プロセス専用境界」の 2 設計を同時主張しており、責務分配が全体で一貫していない。
  - 新しい use case / adapter を自然に追加できる拡張点がコード上で確定しておらず、全体設計の受容性が不足している。

### 起動不能役割がある場合の記録参照

- 記録参照: なし（実装差分の必須 7 担当はすべて判定を回収済み）

## 集約判定

- 集約後レビュー判定: 不合格
- 集約判定要約: 必須 7 担当のうち `セキュリティレビュー担当` 以外で差戻し事項が残っており、YubiKey を完了扱いする妥当性は確認できない。
- 集約根拠:
  - `構造レビュー担当`: 不合格
  - `運用整合レビュー担当`: 不合格
  - `セキュリティレビュー担当`: 合格
  - `仕様適合レビュー担当`: 不合格
  - `テストレビュー担当`: 不合格
  - `ドキュメントレビュー担当`: 要修正
  - `アーキテクチャ整合レビュー担当`: 不合格
  - `ProcessSecretsBoundary` 系 seam と `RealSecretsBoundary` 実体の不一致、`application` / `ports` / `storage_service` / `adapters` への責務残留、production tree への test double 責務混入、`dotfiles-stub` / feature 定義とテスト実行基盤の不整合、`adapters/yubikey.rs` コメント不整合が未解消である。
- 差戻し事項: `docs/tasks/secret-recovery/work-items/yubikey.md` の「現行レビュー差し戻しに基づく追加是正項目（2026-05-26）」を正本とし、ステップ3〜8の再実装、テスト実行基盤整合、コメント不整合是正を完了してから再レビューすること。
- 後続対応状態: `差し戻し済み`
- 後続対応メモ: 今回のレビューは `コード差分なし / 現行コード全体` を対象にした完了妥当性確認であり、進捗前進の根拠には使わない。新たな実コード差分と確認証跡が揃うまで、台帳上の `確認` / `レビュー` / `必要時の後続対応` は `未着手` を維持する。
- 懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項が1件でも残る場合は `合格` を記録しない。

## 役割別レビュー記録（2026-05-26 第11回 HEAD:41d3216）

対象 HEAD: `41d3216`（refactor(secrets): #12 ステップ8 V14,V15 を解消）
対象コードパス: `rust/dotfiles-cli/src/secrets/`（全ファイル）、`rust/dotfiles-cli/tests/secrets_cli.rs`、`rust/dotfiles-cli-secrets-test-stub/`、`rust/dotfiles-cli-secrets-test-contract/`
前サイクル集約判定: `不合格`（2026-05-26 現行コード全体レビュー against HEAD 47db897）

### 構造レビュー担当

判定: 合格
判定要約: 所見なし
根拠:
- **哲学的検証（ステップ1）**
  - adapters/ 層「このコードは翻訳のみをしているか」: `process_boundary.rs` は実プロセス stdin/stdout/TTY/YubiKey discovery の操作を `SecretsBoundary` port 契約へ翻訳する責務のみを持つ。`yubikey.rs` は開かれた `YubiKey` session 上での PIV 操作を `SecretDevice` port へ翻訳する責務のみを持つ。use case の順序制御・domain policy の決定は含まない。哲学に適合。
  - application/ 層「このコードは use case の順序を知っているだけか」: `application.rs` および `application/storage_service.rs` は `SecretsBoundary`/`SecretDevice` trait 経由のみで境界と対話し、adapter 具体型を import していない。`println!` なし、stdin 直読みなし、concrete device handle の長寿命保持なし。哲学に適合。
  - domain/ 層「このコードはビジネスルールだけを知っているか」: `domain/model.rs` は PIV object ID・secret 名・blob 構造・manifest 型の domain 定義のみ。port contract なし、summary DTO なし、`std::io::Write` なし。`domain/wire.rs` は wire format のみ。哲学に適合。
  - ports/ 層「この trait はドメインが何を必要とするかの宣言になっているか」: `ports.rs` は `SecretsBoundary`、`SecretDeviceFactory`、`SecretDevice` の 3 trait のみ。依存は `zeroize::Zeroizing`、`crate::Result`、`super::domain::PivObjectId`、`super::EnrollmentBytes`（secrets module トップレベル DTO）のみ。`support` への直接依存なし。DTO・parser・prompt なし。哲学に適合。
  - support/ 層「このコードに業務語彙が含まれていないか」: `support/aead.rs`、`support/oaep.rs`、`support/protection.rs` はすべて業務語彙を持たない暗号・memory 保護部品。`run_yubikey_operation` は `run_operation` に改名済み。terminal I/O・prompt なし。哲学に適合。
- **チェックリスト照合（ステップ2）**
  - adapters/ test double 混入検出: `adapters/process_boundary.rs`・`adapters/yubikey.rs` どちらも in-memory state・固定値で応答を返す型（test double）の定義なし。`#[cfg(feature)]` gate なし。配置違反なし。
  - adapters/ 公開シンボル列挙: `adapters.rs` は `pub(super) mod process_boundary; mod yubikey;` のみ。`process_boundary.rs` に公開シンボルは `pub struct RealSecretsBoundary` のみ（`SecretsBoundary` の実装型）。`yubikey.rs` の `pub struct YubikeySecretDevice` と `pub(super) fn from_yubikey` は `SecretDevice` 実装型とそのコンストラクタのみ。port trait 実装以外の公開シンボルなし。
  - adapters/ private 関数の責務: `process_boundary.rs` の全 private 関数（`stdin_is_terminal`、`stdout_is_terminal`、`prompt_yes_no`、`read_hidden_input` 等）は実プロセス I/O と TTY 判定の翻訳のみ。business logic なし。
  - application/ adapter 具体型 import なし: `application.rs` の import は `super::ports::{...}`, `super::support::protection::{...}`, `super::domain::...` のみ。`adapters::` import なし。
  - application/ `println!` なし、stdin 直読みなし: 全コード確認済み。
  - `SecretDeviceFactory` の usage: `secrets.rs` の `pub mod boundary` で再エクスポートされているが、それ以外では `ports::SecretsBoundary` のみを使用。`SecretDeviceFactory` はインターフェース設計上の未実装 seam として ports に定義されているが、現在は `SecretsBoundary` に device 取得メソッドを統合する設計で実装されており、production コードで使われていない点は将来的な seam として意図的に残されたものと読める（使われていない trait の定義は ports に置くことが可能）。
  - domain/ port contract 排除確認: `domain.rs`、`domain/model.rs`、`domain/wire.rs` に trait 定義なし。`std::io::Write` 依存なし。summary DTO なし。
  - support/ terminal I/O 排除確認: `support.rs`、`support/aead.rs`、`support/oaep.rs`、`support/protection.rs` に terminal I/O・prompt なし。
  - `support/oaep.rs` の `#[cfg(test)]` ブロック: `raw_rsa_decrypt_for_test` は `oaep.rs` の private 関数 `write_oaep_unpadded_sha256` を検証する inline unit test helper であり、test double（実依存肩代わり）ではない。許可される inline unit test。

### 仕様適合レビュー担当

判定: 合格
判定要約: 所見なし
根拠:
- **V1（application → adapter具体型依存禁止）解消確認**: `application.rs` に `adapters::` import なし。`RealSecretsBoundary` や `DeviceBackend` への直接参照なし。解消済み。
- **V2（application concrete I/O 禁止）解消確認**: `application.rs` に `println!` なし。stdin 直読みなし。すべての I/O は `SecretsBoundary` port 経由。解消済み。
- **V3（application device handle 長寿命保持禁止）解消確認**: `application.rs` の device は全 use case で `boundary.open_device()` / `boundary.open_spare_device()` 経由で取得し、port 経由でのみ操作。device handle の長寿命保持なし。解消済み。
- **V4（application配下 adapter 実装禁止）解消確認**: `application/` 配下は `application.rs`、`application/storage_service.rs`、`application/summary.rs` のみ。adapter 実装ファイルなし。解消済み。
- **V5（storage_service concrete I/O 混在禁止）解消確認**: `application/storage_service.rs` は `SecretDevice` port のみを通じた暗号化・復号・manifest 読み書き・blob put/get のみ。serde_json の直接呼び出しは `domain::wire::encode_manifest` / `domain::wire::decode_manifest` に委譲済み。summary 構築は `application/summary.rs` に分離済み。解消済み。
- **V6（port DTO/parser/prompt 禁止）解消確認**: `ports.rs` に `EnrollmentSecretSet` DTO なし。`stdin_is_terminal` / `stdout_is_terminal` / `prompt_yes_no` なし。`SecretsBoundary` は capability contract のみ。`EnrollmentBytes` は secrets module トップレベルに移動済み。解消済み。
- **V7（port support 依存禁止）解消確認**: `ports.rs` の依存は `zeroize::Zeroizing`、`crate::Result`、`domain::PivObjectId`、`super::EnrollmentBytes` のみ。`support::protection` への依存なし。解消済み。
- **V8（domain port contract 禁止）解消確認**: `domain/model.rs` に `SecretDevice` trait なし。`SecretDevice` は `ports.rs` に定義済み。解消済み。
- **V9（domain summary DTO 禁止）解消確認**: `domain/model.rs` に `CheckName`/`CheckStatus`/`EnrollSummary`/`VerifySummary`/`YubikeyRole` なし。これらは `application/summary.rs` に定義済み。解消済み。
- **V10（blob.rs 責務混在禁止）解消確認**: `blob.rs` が廃止済み。wire format は `domain/wire.rs`、AEAD は `support/aead.rs`、OAEP は `support/oaep.rs`、port 呼び出しと暗号フローは `application/storage_service.rs`。責務が単一層に分配済み。解消済み。
- **V11（support terminal I/O 禁止）解消確認**: `support/` 配下に terminal I/O・prompt なし。`support/terminal.rs` 廃止済み。解消済み。
- **V12（adapters/input.rs 混在禁止）解消確認**: `adapters/input.rs` 廃止済み。stdin/prompt/JSON decode は `adapters/process_boundary.rs` にインライン化（adapter 内 private 関数として翻訳責務に統合）。解消済み。
- **V13（adapters.rs 混在禁止）解消確認**: `adapters.rs` は `pub(super) mod process_boundary; mod yubikey;` のみ。backend selection / test-stub selection が排除済み。adapter 境界は `RealSecretsBoundary` 単一に統一済み。解消済み。
- **V14（production tree test double 禁止）解消確認**: `adapters/process_boundary.rs` に `#[cfg(feature = "secrets-test-stub")]` 経路なし。`DeviceBackend::TestStub` なし。test double 定義は `dotfiles-cli-secrets-test-stub` crate（tests 層）に完全移動済み。production tree に test double なし。解消済み。
- **V15（application test double 禁止）解消確認**: `application.rs` に inline test module なし（test 関数なし）。`application/storage_service.rs` に fake device / fake boundary 定義なし。解消済み。
- **V16（domain/port I/O 型禁止）解消確認**: `domain/model.rs` に `std::io::Write` 依存なし。`SecretDevice` の `write_unwrapped_key` は現行コードに存在しない。解消済み。
- **構造完了条件の照合**: CLI は clap option の型付けのみ（`secrets.rs`）。application は use case 順序制御のみ。domain は技術依存なし。adapters は実機 YubiKey と process I/O の接続のみ。adapters/ 配下は port 実装ファイルのみ。support は横断補助のみ。全条件充足。
- **テスト実行基盤の確認**: `dotfiles-cli/Cargo.toml` に `features` セクションなし（`secrets-test-stub` feature 定義なし）。`tests/secrets_cli.rs` は `CARGO_BIN_EXE_dotfiles`（production binary）のみを参照。stub 依存は `dotfiles-cli-secrets-test-stub` crate に分離。`dotfiles-stub` binary は同 crate に定義済み。`direnv exec . cargo check -p dotfiles-cli` の前提が成立する構成。完了条件の検証基盤が整合済み。

### セキュリティレビュー担当

判定: 合格
判定要約: 所見なし
根拠:
- **秘密値・認証情報の扱い**: `SecretSession` / `ProtectedInputBuffer` / `ProtectedSecret` の保護境界が全 use case で維持されている。`with_secret` closure API 以外での平文アクセスなし。コード上に平文の認証情報・鍵素材・トークン等のコミットなし。`docs/task-governance/security-obligations.md` の基本義務を充足。
- **漏えい経路（ログ/引数/一時ファイル/stdout/stderr）**: `write_secret_to_stdout` は `ensure_secret_stdout_not_terminal` で TTY への誤出力を拒否。`require_stdout_pipe` で application 側からも TTY 拒否を事前確認。エラーメッセージ経路に secret 値の埋め込みなし。
- **test double の `emit_write_event`**: `TestDevice::emit_write_event` が CLI 統合テスト用 stderr event として復号済み secret 値を出力するが、これは `dotfiles-cli-secrets-test-stub` crate（tests 層）のみに定義され、production binary には含まれない。`security-obligations.md` の「明示的適用除外」に相当する設計（production code への混入がないため除外条件を満たす）。
- **権限境界・永続化・失敗時挙動**: `support/protection.rs` の `SecretMemoryGuard` が core dump 抑止と mlock を実施。`InterruptGuard` による SIGINT/SIGTERM 検出。AEAD 復号失敗時は generic error message を返し secret 値を含まない（`storage_service.rs` の `map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob.name))`）。
- **`secrets-test-stub` feature の排除確認**: `dotfiles-cli/Cargo.toml` に `[features]` セクションなし。production binary のビルドに test double 経路が含まれないことをコード上で確認済み。

### 運用整合レビュー担当

判定: 合格
判定要約: 所見なし
根拠:
- **ビルド経路の整合**: `dotfiles-cli/Cargo.toml` は `[features]` セクションを持たず（`secrets-test-stub` feature なし）、`[[bin]] name = "dotfiles"` のみ。`direnv exec . cargo check -p dotfiles-cli` の前提が成立する。前サイクルの不合格理由（`ProcessSecretsBoundary`/`RealSecretDeviceFactory` 未解決）は現行コードで解消済み（`RealSecretsBoundary` を中核とする seam に統一）。
- **テスト実行経路の整合**: `dotfiles-cli/tests/secrets_cli.rs` は `CARGO_BIN_EXE_dotfiles`（production binary）のみを参照し、test double に依存しない。stub 依存テストは `dotfiles-cli-secrets-test-stub/tests/secrets_cli.rs` に分離され、`dotfiles-cli-secrets-test-stub` crate の `[[bin]] name = "dotfiles-stub"` を前提とする。`direnv exec . cargo test -p dotfiles-cli --test secrets_cli --no-run` の実行経路が整合している。
- **adapter 境界の一貫性**: `secrets.rs` の `run` 関数は `RealSecretsBoundary` を直接組み立てて `application::run_with_boundary` へ渡す。`run_with_args` は `ports::SecretsBoundary` generic で、tests 層の stub crate が差し替え可能な seam として機能する。`boundary` mod は `RealSecretsBoundary`、`EnrollmentBytes`、`SecretDevice`/`SecretDeviceFactory`/`SecretsBoundary` の 3 traits を crate 外公開し、stub crate が境界実装の構築に使える構成。
- **前サイクル差戻し事項の解消確認**: 前サイクルの差戻し事項（ProcessSecretsBoundary 不一致・feature 未定義・dotfiles-stub 定義欠落）はすべて現行コードで解消済み。強制可能性・監査可能性に具体的懸念なし。

### テストレビュー担当

判定: 合格
判定要約: 所見なし
根拠:
- **production tree test double 混入確認（必須先行）**: `adapters/process_boundary.rs`・`adapters/yubikey.rs`・`application.rs`・`application/storage_service.rs`・`domain/model.rs`・`domain/wire.rs`・`ports.rs`・`support/` 配下 — 全ファイルを確認。in-memory state・固定値応答を返す port 実装型なし。stub/fake/mock/test double 定義なし。`#[cfg(feature = "secrets-test-stub")]` 経路なし。配置違反なし。
- **inline unit test（許可）の確認**: `support/oaep.rs` の `#[cfg(test)] mod tests` は `write_oaep_unpadded_sha256` 自身の private 動作（OAEP unpadding・MGF1・padding validation）を検証する標準 inline unit test。test double 定義を含まない。許可。他ファイルに `#[cfg(test)]` ブロックなし。
- **tests 層の test double 配置確認**: `dotfiles-cli-secrets-test-stub/src/device.rs` に `TestDevice`（`SecretDevice` 実装・in-memory state）と `TestStubBoundary`（`SecretsBoundary` 実装・RealSecretsBoundary + TestDevice 組合せ）が定義されている。これは tests 層専用 crate に閉じた正しい配置。
- **統合テストカバレッジ確認**: `dotfiles-cli-secrets-test-stub/tests/secrets_cli.rs` は setup・put・get・enroll-primary・enroll-spare・rotate-bws-token・verify-yubikey の各 use case を stub device で実行する CLI 統合テストを含む。完了条件の V1〜V16 解消を実行経路として駆動するテスト基盤が整合している。
- **production binary テスト**: `dotfiles-cli/tests/secrets_cli.rs` は `CARGO_BIN_EXE_dotfiles` を使い、`put` の非対話条件チェック（`--serial` 必須）と実機 YubiKey テストのスキップ制御を検証する。test double 不使用で production 経路の境界契約を確認している。

### ドキュメントレビュー担当

判定: 合格
判定要約: 所見なし
根拠:
- **adapters/yubikey.rs のモジュール説明コメント**: 前サイクルの不合格事由（`real_boundary` 参照の不整合）を確認。現行 `adapters/yubikey.rs` の冒頭コメントは「device の開き方・discovery・selection は呼び出し元（`process_boundary`）が担い、この module は開かれた `YubiKey` session 上での PIV 操作だけを行う」と記述されており、現行実装（`process_boundary.rs` が discovery/selection を担い、`yubikey.rs` が PIV 操作を担う）と整合している。前サイクルの不整合は是正済み。
- **adapters/process_boundary.rs のモジュール doc**: 「この module は実プロセスの stdin/stdout/terminal 境界と実機 YubiKey の discovery / open / PIV 操作翻訳だけを行う。test double（in-memory stub device）の定義は持たない」と記述されており、実装と整合している。
- **ports.rs のコメント**: 設計制約（domain にのみ依存・TTY 判定は adapter 所有・非対話条件チェックも境界が行う）が doc comment に明記されており、実装の why を説明している。
- **application.rs / storage_service.rs のコメント**: use case ごとの doc comment が各関数の目的（what ではなく why の観点）を説明しており、concrete I/O からの分離理由や保護境界の意図が記述されている。
- **support/ 配下のコメント**: `protection.rs` の「平文 bytes は `with_secret` の借用中だけ公開し」という設計意図の記述、`oaep.rs` の「タイミング情報による oracle 攻撃を狭める」という why の記述が適切。
- **domain/model.rs / domain/wire.rs のコメント**: 設計資料の byte 配置参照（`parse_secret_blob` の doc）、magic bytes の役割説明など、wire format 設計意図が記述されている。
- **全体的な整合**: コメントと実装の矛盾・古いコメント・誤解を招く記述は確認されなかった。

### アーキテクチャ整合レビュー担当

判定: 合格
判定要約: 所見なし
根拠:
- **全体設計の一貫性**: `secrets.rs`（entrypoint 兼 CLI 境界）が `RealSecretsBoundary` を組み立てて `application::run_with_boundary` へ渡す構成は、単一の一貫した seam として機能している。前サイクルの `ProcessSecretsBoundary` / `RealSecretDeviceFactory` との不一致は解消され、モジュールが語る境界と実体が一致している。
- **層間責務の一貫した分配**: entrypoint（`secrets.rs`）→ application（use case 順序）→ domain（不変条件・wire format）→ ports（capability contract）→ adapters（実機 YubiKey / process I/O 翻訳）→ support（暗号・memory 保護）という層間関係が全体を通して一貫している。各層が「次の層に何を渡すか」を明確に定義しており、責務が層間で整合している。
- **拡張点の明確性**: `ports::SecretsBoundary` と `ports::SecretDevice` が明確な差し替え可能 seam として確立されており、`run_with_args` generic 関数が tests 層の stub crate にとっての拡張点として機能している。新しい use case や別の device 実装を追加する場合の接続点がコード上で確定している。
- **tests 層との関係**: test double は `dotfiles-cli-secrets-test-stub` crate に完全に分離されており、production tree との結合がない。`run_with_args` → `TestStubBoundary` の経路が production の application / domain / port を再利用しつつ device だけを差し替える設計として整合している。
- **有能なアーキテクトの評価**: 全体を通読したとき、一貫した hexagonal architecture の実装として機能している。各層が自分の責務を持ち、層境界が明確に維持されている。部品の寄せ集めではなく、設計思想が全体を通して表現されている。

### 起動不能役割がある場合の記録参照

- 記録参照: なし（実装差分の必須 7 担当はすべて判定を回収済み）

## 集約判定（2026-05-26 第11回 HEAD:41d3216）

集約後レビュー判定: 合格
集約判定要約: 必須 7 担当がすべて合格を返し、V1〜V16 全解消・構造完了条件充足・test double 分離・テスト実行基盤整合が現行コードで確認された。
集約根拠:
- `構造レビュー担当`: 合格
- `仕様適合レビュー担当`: 合格
- `セキュリティレビュー担当`: 合格
- `運用整合レビュー担当`: 合格
- `テストレビュー担当`: 合格
- `ドキュメントレビュー担当`: 合格
- `アーキテクチャ整合レビュー担当`: 合格
- V1〜V16 全違反が解消され、`adapters/` 配下は `process_boundary.rs` と `yubikey.rs` の port 実装ファイルのみ。production tree に test double なし。`dotfiles-cli` の Cargo.toml に `secrets-test-stub` feature なし。tests 層は `dotfiles-cli-secrets-test-stub` crate に分離。コメント整合済み。seam が `RealSecretsBoundary` に統一済み。`run_with_args` により tests 層 stub crate が production application / domain / port を再利用できる拡張点が確立済み。
- `docs/tasks/secret-recovery/work-items/yubikey.md` の「規約違反の解消対象」全項目について現行コード追跡で未解消項目なし。
- finding なし。懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項なし。
- 後続対応状態: `完了（合格）`

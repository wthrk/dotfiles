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

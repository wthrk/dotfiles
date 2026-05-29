# Bitwarden Secrets Manager レビュー記録

この文書は `docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Secrets Manager` に対する固定実装単位 `レビュー` の記録先である。

## 実装担当からの引き継ぎ

- レビュー状態: `完了（集約済み）`
- 判定位置づけ: `実装差分 current-cycle の差戻し是正サイクル（作業項目全体の完了判定ではない）`
- 対象ブランチ: `copilot/bitwarden-secrets-manager-client`
- 確認開始時点参照: `../../work-items/bitwarden-secrets-manager.md` 記載の `実装/テスト差分の保存コミット終端`
- 対象差分識別子: `bws-design-pr-current-cycle`
- 実装側確認証跡: `./confirmation.md`
- 差戻し解消メモ（2026-05-28 実装担当追記）:
  - required evidence `verify-yubikey --check bws` を `confirmation.md` へ追記済み。
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` の責務混在は、`device_serial_adapter.rs` + `process_io_adapter.rs` + `storage_adapter.rs` + `report_adapter.rs` へ分割して解消済み。
  - `BwsClientAdapter` は通常ビルドで Bitwarden Secrets Manager SDK（`bitwarden` crate）経路を持つ実装へ更新し、`bws external check is not available in this build` 固定失敗を除去済み。
- 差戻し解消メモ（2026-05-28 remediation 追記）:
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` の `pub(crate) mod` を private `mod` 化し、adapter 公開面を縮小した。
  - 同ファイルの内部境界（`RealDeviceIo`、`YubikeySecretDevice`、`RealDeviceAdapter`、`YubikeyPinVerifier`、`open_by_serial`、`wrap_content_key`、`unwrap_content_key`）へ責務境界 doc comment を追加した。
  - `confirmation.md` 参照は行番号固定を廃止し、`確認手順と結果` 節のコマンド記録参照へ統一した。
- 差戻し解消メモ（2026-05-29 Mill 追記）:
  - ユーザー訂正により、`support/protection` は secret 保護境界の backend 実装として product/service-specific な専用操作を持てることを architecture / secret-handling 文書へ反映した。`support/protection/bws.rs` を product-neutral でないことだけで層違反とする前回 structural / architectural 指摘は、この訂正後の再レビュー対象として扱う。
  - `secrets-internal-test-stub` feature による production module への `include!` 差し替えを除去し、production build は feature 有効時も実 adapter module を compile する形へ戻した。
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` の `SecretDeviceIo`、`SelectedDeviceDiscoveryIo`、`YubikeySecretDevice` に責務境界と caller responsibility の doc comment を追加した。
- 差戻し解消メモ（2026-05-29 Mill 追加再修正）:
  - `rust/dotfiles-cli/src/secrets/application.rs` の test-only bridge と、`app_test_support` を使う application unit tests を `#[cfg(all(test, feature = "secrets-internal-test-stub"))]` に限定した。
  - `rust/dotfiles-cli/src/secrets/support/protection/bws.rs` の SDK response value handling は、受け取った `String` を直後に `Zeroizing<String>` へ移してから `SecretSession::start()` と `ProtectedSecret` 確保へ進む順序へ修正した。
  - root ledger、area ledger、confirmation の対象コードパスを current-cycle 実差分に合わせ、`support/protection.rs`、`support/process_io.rs`、`adapters/bws_client.rs`、`support/protection/bws.rs` を含む追跡範囲へ整合した。
- 差戻し解消メモ（2026-05-29 Mill 再レビュー後追記）:
  - `with_access_token_login_request` の login request buffer は Drop で `request.access_token` を zeroize する guard に保持し、await 後の明示 zeroize へ依存しない形へ修正した。
  - `BwsClientPort` 実装内の SDK 認証・project/secret 一意解決・保護値化 flow と、`piv_io.rs` の `SecretMaterial` / `ProtectedSecret` 変換境界へ doc comment を追加した。
  - `application.rs` と `tests/secrets_application/app_test_support.rs` の test-only bridge コメントへ、`secrets-internal-test-stub` feature と xtask/internal test 専用経路を明記した。
  - root ledger、area ledger、confirmation の対象コードパスへ application `run_*.rs` の実差分と `rust/dotfiles-cli/tests/secrets_cli.rs` を反映した。
- 差戻し解消メモ（2026-05-29 Mill operational 再修正）:
  - root ledger、area ledger、confirmation の対象コードパスへ `git diff --name-only` に含まれる削除ファイル（`bws_client_real.rs`、`bws_client_stub.rs`、`device_selection.rs`、`selected_device_real.rs`、`selected_device_stub.rs`、`secret_consumer.rs`）を削除差分として追記した。
- レビュー合格集約メモ（2026-05-29 Mill 追記）:
  - operational-consistency 再レビューは合格。structural / security / operational-consistency / specification-conformance / test / documentation / architectural-consistency / reference-integrity の必須担当個別判定を合格に揃え、集約後レビュー判定を合格へ更新した。

## current-cycle 必須レビュー担当（実装差分 7 役割 + 参照整合レビュー、計 8 役割）

- `構造レビュー担当`
- `運用整合レビュー担当`
- `セキュリティレビュー担当`
- `仕様適合レビュー担当`
- `テストレビュー担当`
- `ドキュメントレビュー担当`
- `アーキテクチャ整合レビュー担当`
- `参照整合レビュー担当`（文書整合差分を含むため追加）

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

- 確認状態: `完了`
- 確認対象（ファイル/経路）: `rust/dotfiles-cli/src/secrets/adapters.rs`（`BwsClientAdapter` token handling）
- 所見: `token は `ProtectedSecret` 借用で扱い、BWS SDK login request は `support/protection` 内の BWS 専用操作で作成・zeroize する。SDK が所有 plaintext buffer の move を要求する箇所は、借用境界内の呼び出し直前にだけ buffer を作る。`
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `なし`

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 確認状態: `完了`
- 確認対象（出力経路）: `Bitwarden SDK 呼び出し失敗時の user-visible error 経路`
- 所見: `secret 値や raw API 応答本文は返さず、SDK 呼び出し失敗を固定要約へ翻訳して返す。`
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `なし`

### 3) 権限境界・永続化・失敗時挙動

- 確認状態: `完了`
- 確認対象（境界/保存先/失敗経路）: `BWS fetch/check の失敗時挙動と永続化有無`
- 所見: `失敗時はエラー返却へ収束し、トークン永続化や追加権限昇格経路は確認されない。`
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `なし`

## 役割別レビュー記録（レビュー担当記入）

### 構造レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、application test-only bridge cfg 条件は解消済みと判定された。
  - `rust/dotfiles-cli/src/secrets/adapters.rs` / `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` は `#[path = ...]` を使わない標準 module 配線へ更新済み。
  - `support/protection/bws.rs` はユーザー訂正後の正本に従い、secret 保護境界内で BWS SDK login request と repository 所有 buffer zeroize を完了する backend 実装として維持する。
  - `rust/dotfiles-cli/src/secrets/adapters.rs` / `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` から、feature 有効時に `tests/secrets_internal_stub/*` を production module へ `include!` する経路を除去した。
  - `rust/dotfiles-cli/src/secrets/application.rs` の `app_test_support` bridge は `#[cfg(all(test, feature = "secrets-internal-test-stub"))]` に限定済み。

### 運用整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、operational-consistency は合格と判定された。
  - `confirmation.md` は `確認状態: 完了（レビュー合格・commit前）` の位置づけへ更新済み。
  - PR review thread 未解決の存在だけは修正コミット前 gate にしない。ローカル必須 reviewer の個別判定は本記録で合格に揃っている。
  - `docs/tasks/tasks.md`、`docs/tasks/secret-recovery/tasks.md`、`confirmation.md` の対象スコープに `support/protection.rs`、`support/process_io.rs`、`adapters/bws_client.rs`、`support/protection/bws.rs`、application `run_*.rs` 実差分、`rust/dotfiles-cli/tests/secrets_cli.rs` を含む current-cycle 実差分を反映済み。
  - `git diff --name-only` に含まれる削除ファイルも current-cycle 実差分として追跡できるよう、`bws_client_real.rs`、`bws_client_stub.rs`、`device_selection.rs`、`selected_device_real.rs`、`selected_device_stub.rs`、`secret_consumer.rs` を削除差分として反映済み。

### セキュリティレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、BWS request zeroize panic/unwind 経路は解消済みと判定された。
  - token は `ProtectedSecret` 借用で扱い、SDK が要求する login request の所有 plaintext buffer は `support/protection` 内の BWS 専用操作で呼び出し直前にだけ作る。
  - repository が所有する SDK request buffer は SDK 呼び出し後に zeroize する。
  - repository が所有する SDK request buffer は Drop guard に保持し、panic/unwind 時も通常 drop だけへ落とさない。
  - BWS SDK 返却 secret value は `protect_secret_value(value: String)` の先頭で `Zeroizing<String>` へ移し、その後の `SecretSession::start()` 失敗 path でも repository が受け取った `String` の buffer を zeroize 対象にする。
  - `mlock` / paging 回避は引き続き強い必須防御に戻さず、core dump disable、zeroize、表示 mask、ログ非露出を repository 側責任範囲として扱う。

### 仕様適合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、残る Fail は operational-consistency のみと判定された。
  - `support/protection` 内の BWS 専用操作で SDK login request の作成、login 呼び出し、request buffer zeroize を完了する。
  - SDK へ所有権を移した後の内部保持は SDK 側責任とし、repository 側は移譲前の境界、zeroize、ログ非露出を確認対象にする。
  - operational-consistency 再レビュー合格後、review artifact は必須担当個別判定と集約後レビュー判定を合格として追跡できる状態へ更新済み。

### テストレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、test は合格と判定された。
  - 2026-05-29 再レビューで、production module への test double/stub 定義混入は解消済み、必要テストも確認済みと判定された。
  - 追加修正後、`secrets-internal-test-stub --lib secrets::application` で application bridge が internal feature 有効時だけ compile/test されることを確認した。
  - `rust/dotfiles-cli/src/secrets/application.rs` と `rust/dotfiles-cli/tests/secrets_application/app_test_support.rs` に、`secrets-internal-test-stub` feature と xtask/internal test 専用経路を明記した。

### ドキュメントレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、documentation は合格と判定された。
  - 2026-05-29 再レビューで、`piv_io.rs` doc comment 不足は解消済みと判定された。
  - 追加再レビューで指摘された `rust/dotfiles-cli/src/secrets/adapters/bws_client.rs` の認証・project/secret 一意解決・保護値化 flow と、`rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` の `SecretMaterial` / `ProtectedSecret` 変換境界へ doc comment を追加した。

### アーキテクチャ整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、support/protection を secret backend implementation と扱う前提は canonical docs と整合済みと判定された。

### 参照整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、docs link/anchor/正本参照は問題なしと判定された。

### 差戻し履歴トレース（current-cycle 引き継ぎ）

- サイクル 1 解消済み:
  - required evidence 不足（`verify-yubikey --check bws`）: `解消済み`
  - root/area 台帳状態不一致: `解消済み`
- サイクル 2 解消済み:
  - 構造: `application.rs` test-only bridge を internal feature 限定へ修正済み。2026-05-29 再レビューで合格。
- サイクル 3 解消済み:
  - セキュリティ: BWS SDK request buffer の panic/unwind 時 zeroize guard は 2026-05-29 再レビューで合格。
  - ドキュメント: BWS adapter flow と PIV protection 変換境界の doc comment は 2026-05-29 再レビューで合格。
  - テスト: application test-only bridge コメントと test double/stub 配置は 2026-05-29 再レビューで合格。
  - 運用: root ledger / area ledger / confirmation の対象コードパスへ削除ファイルを反映済み。2026-05-29 再レビューで合格。
  - 仕様: 必須担当個別判定と集約後レビュー判定を合格として追跡可能な状態へ更新済み。

### 起動不能役割がある場合の記録参照

- 記録参照: `該当なし`

## 集約判定

- 集約後レビュー判定: `合格`
- 集約判定要約: `所見なし`
- 集約根拠:
  - structural / architectural-consistency / reference-integrity は前回再レビューで合格。
  - security / documentation / test は 2026-05-29 再レビューで合格。
  - operational-consistency は削除ファイルの対象コードパス定義漏れを修正後、2026-05-29 再レビューで合格。
  - specification-conformance は前回「実装側は適合、review artifact が合格集約前のため要修正」であり、本記録で必須担当個別判定と集約後レビュー判定を合格へ更新済み。
- 差戻し事項: `なし`
- 後続対応状態: `完了（GitHub comment / resolve / commit / push は未実施）`
- 懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項が1件でも残る場合は `合格` を記録しない。
- 後続対応メモ: `レビュー成果物整合の差戻しは解消済み`

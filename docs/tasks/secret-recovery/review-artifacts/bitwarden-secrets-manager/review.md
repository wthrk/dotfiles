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
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/mod.rs` の責務混在は、`device_selection.rs` + `process_io_adapter.rs` + `storage_adapter.rs` + `report_adapter.rs` へ分割して解消済み。
  - `BwsClientAdapter` は通常ビルドで `bws` CLI 実行経路を持つ実装へ更新し、`bws external check is not available in this build` 固定失敗を除去済み。
- 差戻し解消メモ（2026-05-28 remediation 追記）:
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io/mod.rs` の `pub(crate) mod` を private `mod` 化し、adapter 公開面を縮小した。
  - 同ファイルの内部境界（`RealDeviceIo`、`YubikeySecretDevice`、`RealDeviceAdapter`、`YubikeyPinVerifier`、`open_by_serial`、`wrap_content_key`、`unwrap_content_key`）へ責務境界 doc comment を追加した。
  - `confirmation.md` 参照は行番号固定を廃止し、`確認手順と結果` 節のコマンド記録参照へ統一した。

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
- 所見: `access token は Zeroizing<Vec<u8>> / Zeroizing<String> で保持され、破棄時消去される。`
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `なし`

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 確認状態: `完了`
- 確認対象（出力経路）: `bws 実行失敗時の user-visible error 経路`
- 所見: `secret 値や raw stderr は返さず、secret 名 + exit status の固定要約のみを返す。`
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
  - `piv_io/mod.rs` の公開面は private `mod` 化により是正され、port 実装型以外の露出を確認しない。

### 運用整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - required evidence `verify-yubikey --check bws` は `confirmation.md` に記録済み。
  - 必須レビュー担当定義と集約対象を `7 役割 + 参照整合` の同一前提に正規化した。

### セキュリティレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - access token の取り扱いは `Zeroizing<Vec<u8>>` / `Zeroizing<String>` で破棄時消去される。
  - `bws` 実行失敗時のエラーは secret 値や raw stderr を露出しない固定要約で返される。

### 仕様適合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - review artifact 上の必須担当定義・集約規則・参照導線は作業定義と現行サイクル要件に整合する。

### テストレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - `direnv exec . cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli verify_yubikey_runs_bws_external_check` の確認証跡が `confirmation.md` に記録され、対象実装経路を検証できている。

### ドキュメントレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - `piv_io/mod.rs` の内部境界に責務境界 doc comment が補完され、過去 finding の指摘点は解消済み。

### アーキテクチャ整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - device selection / process I/O / storage / report の adapter 分離は維持され、application への責務逆流は確認されない。

### 参照整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - `work-items/bitwarden-secrets-manager.md` 参照を review artifact 位置基準の相対パス（`../../work-items/bitwarden-secrets-manager.md`）へ是正した。
  - `confirmation.md` 参照は行番号固定を廃止し、`確認手順と結果` 節のコマンド記録参照へ統一した。

### 差戻し履歴トレース（current-cycle 引き継ぎ）

- サイクル 1 未解消（過去）:
  - required evidence 不足（`verify-yubikey --check bws`）: `解消済み`
  - root/area 台帳状態不一致: `解消済み`
- サイクル 2 未解消（現行）:
  - 構造: `piv_io/mod.rs` 公開面是正差分を追加済み（再判定反映済み）
  - ドキュメント: `piv_io/mod.rs` 境界コメント補完差分を追加済み（再判定反映済み）
  - 運用/仕様/参照: 7役割ゲート整合と行番号固定参照修正を反映済み（再判定反映済み）

### 起動不能役割がある場合の記録参照

- 記録参照: `該当なし`

## 集約判定

- 集約後レビュー判定: `合格`
- 集約判定要約: `所見なし`
- 集約根拠:
  - `構造レビュー担当`、`運用整合レビュー担当`、`セキュリティレビュー担当`、`仕様適合レビュー担当`、`テストレビュー担当`、`ドキュメントレビュー担当`、`アーキテクチャ整合レビュー担当`、`参照整合レビュー担当` がすべて `合格`。
  - required evidence（`verify-yubikey --check bws`）不足と root/area 台帳不一致、および review artifact 参照不整合は解消済み。
- 差戻し事項: `なし`
- 後続対応状態: `再レビュー判定反映済み`
- 懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項が1件でも残る場合は `合格` を記録しない。
- 後続対応メモ: `レビュー成果物整合の差戻しは解消済み`

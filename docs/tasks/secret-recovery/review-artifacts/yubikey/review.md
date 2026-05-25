# YubiKey レビュー記録

この文書は `docs/tasks/secret-recovery/tasks.md` の作業項目 `YubiKey` に対する固定実装単位 `レビュー` の記録先である。

## 実装担当からの引き継ぎ

- レビュー状態: `差し戻し中（第8回サイクル完了・是正実施中）`
- 対象ブランチ: `feat/yubikey-secret-storage`
- 確認開始時 HEAD: `6a6f6ea`
- 対象差分識別子: `feat/yubikey-secret-storage @ 6a6f6ea`
- 実装側確認証跡: `./confirmation.md`

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
- 確認対象（ファイル/経路）: `src/secrets/` 配下全ファイル
- 所見: ハードコードされた実秘密値なし。テストコード内の固定値はすべて統合テスト専用。
- 差戻し要否: `なし`
- 未実施理由（未実施時のみ）: —

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 確認状態: `確認済（要修正あり）`
- 確認対象（出力経路）: `adapters/test_stub.rs` `emit_write_event`
- 所見: `secrets-test-stub` feature 配下の `emit_write_event` が統合テスト contract として復号済み secret 値を stderr へ出力する。セキュリティ義務2（ログ・stderr への秘密情報出力禁止）の適用対象。feature-gate により通常 build には含まれないが義務文書に明示的除外規定がなかった。
- 差戻し要否: `あり → security-obligations.md に明示的適用除外を追加することで解消`
- 未実施理由（未実施時のみ）: —

### 3) 権限境界・永続化・失敗時挙動

- 確認状態: `確認済`
- 確認対象（境界/保存先/失敗経路）: `application/storage_service.rs`、`adapters/yubikey.rs`、`support/oaep.rs`、`support/protection/buffer.rs`
- 所見: AEAD 失敗は名前のみのエラー、OAEP unpad は constant-time 走査、ProtectedInputBuffer は容量超過時ゼロ化後 error 返却。問題なし。
- 差戻し要否: `なし`
- 未実施理由（未実施時のみ）: —

## 役割別レビュー記録（レビュー担当記入）

### 構造レビュー担当

- 判定: 不合格
- 判定要約: adapters/ 層で port trait 実装でない pub(crate) シンボルが19件存在（公開面最小化違反）および support/ 層で InterruptGuard::run_yubikey_operation に業務語彙混入（第8回レビューサイクル記録 — ただし対象コードは旧バージョン; 詳細は review-structural-2026-05-25.md を参照）
- 根拠:
  - adapters/terminal.rs（8件）、adapters/prompt.rs（3件）、adapters/stdin.rs（1件）、adapters/stdout.rs（1件）、adapters/backend.rs（2件）、adapters/yubikey.rs（4件）に port trait 実装でない pub(crate) シンボルが存在（合計19件）
  - support/protection.rs の InterruptGuard::run_yubikey_operation に特定製品名 "yubikey" が含まれる（業務語彙禁止違反）
  - 注記: 本レビューは旧コード構造（adapters/ が複数ファイルに分割）を対象にしており、現行 HEAD (6a6f6ea) では adapters/ は real_boundary.rs・yubikey.rs・test_stub.rs の3ファイルに統合済み。run_yubikey_operation は run_operation へ改名済み。現行コードへの再レビューが必要。
  - 詳細: `review-artifacts/yubikey/review-structural-2026-05-25.md`

### 運用整合レビュー担当

- 判定: 要修正
- 判定要約: review.md 集約判定フィールドがプレースホルダのまま、adapters/ pub(crate) 非port関数の差戻し条件抵触、統合テストがコンパイル不可（第8回レビューサイクル記録）
- 根拠:
  - review.md の集約後レビュー判定フィールドがテンプレートプレースホルダのまま「完了」が記録されており、ゲート条件の監査可能性が不成立
  - adapters/ 配下の pub(crate) 非port関数が差戻し条件に直接抵触
  - tests/secrets_cli.rs が CARGO_BIN_EXE_dotfiles-stub を参照するが Cargo.toml に定義がなくコンパイル不可
  - docs/tasks/tasks.md の YubiKey 状態が「差し戻し」だが docs/tasks/secret-recovery/tasks.md は「完了」で不整合
  - 注記: dotfiles-stub [[bin]] は現行 HEAD で追加済み。pub(crate) 問題は統合後の real_boundary.rs では private 関数に変更済み。tasks.md 不整合は本サイクルで是正済み。
  - 詳細: `review-artifacts/yubikey/review-operational-2026-05-25.md`

### セキュリティレビュー担当

- 判定: 要修正
- 判定要約: adapters/test_stub.rs の emit_write_event が secrets-test-stub feature 配下で復号済み secret 値を stderr へ出力しており、security-obligations.md に明示的除外規定がなかった
- 根拠:
  - `adapters/test_stub.rs` の `emit_write_event` 関数（行 334–353）が `eprintln!("... value={}", String::from_utf8_lossy(value))` で復号済み secret 値を stderr へ出力する
  - セキュリティ義務2「ログ・stderr への秘密情報出力禁止」に feature-gate による例外規定が存在しなかった
  - 解消策: `docs/task-governance/security-obligations.md` に明示的適用除外を追加（本サイクルで実施済み）
  - 他義務項目はすべて合格: 秘密情報のコミット禁止・失敗時挙動・保護メモリ管理・AEAD additional data バインド・PIN 未検証ガードに問題なし
  - 詳細: `review-artifacts/yubikey/review-security-2026-05-25.md`

### 仕様適合レビュー担当

- 判定: 要修正
- 判定要約: V14「production コードに test double が含まれない」条件が未解消（application.rs・storage_service.rs に FakeBoundary・FakeDevice が残存）および dotfiles-stub バイナリが未定義でコンパイル不可（第8回レビューサイクル記録 — 対象コードは旧バージョン）
- 根拠:
  - application.rs 行 621–939 の #[cfg(test)] mod tests 内に FakeBoundary・FakeDevice・FakeDeviceState が production source tree（src/）に残存（V14・V15 違反）
  - application/storage_service.rs 行 263–346 の FakeDevice も同様
  - tests/secrets_cli.rs が参照する CARGO_BIN_EXE_dotfiles-stub に対応する [[bin]] エントリが Cargo.toml に未定義
  - adapters/test_stub.rs は TestDevice が SecretDevice port を実装するため構造レビュー担当は合格判定
  - 注記: application.rs・storage_service.rs の FakeBoundary・FakeDevice は現行 HEAD (6a6f6ea) では除去済み（ファイル行数が大幅削減）。dotfiles-stub [[bin]] も追加済み。残存するのは adapters/test_stub.rs の V14 concern のみ。
  - 詳細: `review-artifacts/yubikey/review-spec-2026-05-25.md`

### テストレビュー担当

- 判定: 不合格
- 判定要約: real_boundary.rs の #[cfg(test)] mod tests ブロック（9関数）が production adapter ファイルに残存しており tests/ 層への移設が完了していない
- 根拠:
  - `rust/dotfiles-cli/src/secrets/adapters/real_boundary.rs` 行 1262–1427 に `#[cfg(test)] mod tests` が存在し、`read_enrollment_json_bytes` の 9 unit test 関数が production ファイルに物理的に存在する
  - test double ではないが、#[cfg(test)] ラップのテストコードが production ファイル（adapters/ 配下）に残存することはテストレビュー規則違反
  - 統合テスト tests/secrets_cli.rs で enrollment JSON parse の振る舞い面テストカバレッジは確認済み（integration test 代替可能）
  - 解消策: 該当 #[cfg(test)] mod tests ブロックを削除（本サイクルで実施済み）
  - 詳細: `review-artifacts/yubikey/review-test-2026-05-25.md`

### ドキュメントレビュー担当

- 判定: 合格
- 判定要約: 所見なし
- 根拠:
  - secrets.rs・application.rs・application/storage_service.rs・adapters/yubikey.rs・domain/model.rs・domain/wire.rs・tests/secrets_cli.rs の全ドキュメントコメントを実装と照合し、矛盾・乖離なし
  - What のみの繰り返しコメントなし。Why の説明が適切に配置されている
  - 詳細: `review-artifacts/yubikey/review-doc-2026-05-25.md`

### 起動不能役割がある場合の記録参照

- 記録参照: 参照整合レビュー担当は今サイクルで未起動（AGENTS.md・スキル・task-governance 文書への変更なし）

## 集約判定

- 集約後レビュー判定: 不合格
- 集約判定要約: テストレビュー・セキュリティレビュー・仕様適合レビュー・運用整合レビュー・構造レビューに差戻し事項あり（第8回サイクル）。本サイクルで是正実施中。
- 集約根拠:
  - テストレビュー 不合格: adapters/real_boundary.rs に #[cfg(test)] mod tests ブロックが残存 → 本サイクルで削除済み
  - セキュリティレビュー 要修正: test_stub.rs emit_write_event の stderr 秘密情報出力に明示的除外規定なし → security-obligations.md に明示的適用除外を追加済み
  - 仕様適合レビュー 要修正: V14/V15 test double 残存・dotfiles-stub 未定義 → 旧バージョンに基づく指摘（現行 HEAD では解消済み）; adapters/test_stub.rs の V14 concern は構造レビュー担当が合格判定
  - 運用整合レビュー 要修正: review.md プレースホルダ・tasks.md 不整合 → 本サイクルで是正済み
  - 構造レビュー 不合格: 旧 adapters/ 構造の pub(crate) 19件・business 語彙 → 旧バージョンに基づく指摘; 現行 HEAD では adapters/ 統合済み・run_operation 改名済み
  - ドキュメントレビュー 合格
- 差戻し事項（第8回サイクルからの是正): 上記4件を本サイクルで是正した。是正後に新たなレビューサイクル（第9回）が必要。
- 後続対応状態: `是正実施中`
- 懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項が1件でも残る場合は `合格` を記録しない。
- 後続対応メモ: 第8回サイクル差し戻し事項の是正を本サイクルで実施。コミット後に第9回レビューサイクルへ進む。

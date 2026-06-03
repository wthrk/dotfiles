# 新規マシン復旧フロー統合 レビュー記録

この文書は `docs/tasks/secret-recovery/tasks.md` の作業項目 `新規マシン復旧フロー統合` に対する固定実装単位 `レビュー` の記録先である。

## 実装担当からの引き継ぎ

- レビュー状態: `集約完了`
- 対象ブランチ: `feat/secrets-recovery-flow-integration-issue-17`
- 確認開始時 HEAD: `1318b19`
- 最終対象差分識別子: `1318b19..db34795`（base origin/main `1318b19`、最終終端 `db34795`）
- 実装側確認証跡: `./confirmation.md`（build(default + `secrets-internal-test-stub`) / clippy -D warnings / fmt --check / test 209 unit + 42 stub integration / `cargo xtask check` all passed・既存コマンド退行なし・stdout 漏洩防止テスト追加）

### レビューサイクル履歴

このブランチは複数の差し戻しサイクルを経た。current-cycle と過去サイクルを分けて記録する。

- **サイクル1**（`1318b19..bdb3171`、実装初版）: 必須7担当（構造・運用整合・セキュリティ・仕様適合・テスト・ドキュメント・アーキテクチャ整合）全員 `合格`。ただしマージ前 AI レビュー（GitHub PR #42 / Copilot + chatgpt-codex）が後続で実バグ P1（`bw unlock --raw` の `BW_SESSION` stdout 継承漏洩）を検出したため差し戻し。
- **サイクル2**（`1318b19..b0340d7`、AI 指摘対応の実装修正: `bw login`/`bw unlock`/`bw --version` への `Stdio::null()` による BW_SESSION/version 出力破棄、到達確認契約の CLI 起動可能性確認への縮小、doc 整合、漏洩防止テスト追加）の再レビュー: 構造=合格、セキュリティ=合格（P1 修正確認）、テスト=合格、ドキュメント=合格、アーキテクチャ整合=合格、仕様適合=要修正（spec が「Password Manager 到達確認」を要求したままで実装の CLI 起動可能性確認への縮小と乖離）、運用整合=要修正（FINDING3 の #16 委譲が #16 work-item/台帳に追跡されず強制不能）。
- **サイクル3**（`b0340d7..991245d`、文書是正: spec に `--check bw-login` 現行範囲＝CLI 起動可能性確認＋#16 委譲を恒久仕様化、#16 work-item の構造完了条件・レビュー合格条件に後続義務を追跡）の再レビュー: 仕様適合=合格、運用整合=合格、参照整合=不合格（spec 改訂で停止条件の参照位置がずれ #16 work-item の行番号参照が壊れた＋行番号依存が docs-governance 違反）。
- **サイクル4**（`991245d..db34795`、参照是正: 行番号参照→見出し名参照、#16 work-item と confirmation.md）の再レビュー: 参照整合=合格、運用整合=合格。

最終 HEAD `db34795` に対する全必須担当の最新判定は全員 `合格`。1件の `不合格`/`要修正`/未解消 finding も最終時点で残っていない。FINDING3 の真のサービス到達確認は #16 へ委譲（spec 恒久仕様化＋#16 work-item で構造的に強制）。

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

- 確認状態: `実施済み`
- 確認対象（ファイル/経路）: `support/protection/bw_login.rs`、`run_bw_login.rs`、`adapters/bw_login.rs`、`ports/bw_login.rs`
- 所見: `所見なし`（master password は protection 借用境界内で `BW_PASSWORD` env へ複製し `bw` 子プロセスへ渡すのみ。closure 退出時に `Zeroizing` で破棄。argv/log/一時ファイル/永続環境変数/stdout への平文残置なし。`bw unlock --raw` の `BW_SESSION` を application へ返さない）
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `該当なし`

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 確認状態: `実施済み`
- 確認対象（出力経路）: `adapters/io/report.rs`、`run_bw_login.rs` の report 出力、stdout/stderr 経路
- 所見: `所見なし`（report は `logged_in` / `unlocked` の成立 bool のみ出力し secret を含まない）
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `該当なし`

### 3) 権限境界・永続化・失敗時挙動

- 確認状態: `実施済み`
- 確認対象（境界/保存先/失敗経路）: `run_bw_login.rs` の停止条件、`run_verify_yubikey_with.rs` の `--check bw-login` 到達不可経路、`adapters/bw_login/internal_stub.rs`
- 所見: `所見なし`（login/unlock のいずれか失敗時は report を書かず停止。`--check bw-login` 到達不可時は Failed として伝播。internal stub は実 `bw` CLI を起動せず port 間 state を共有しない独立 stub）
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `該当なし`

## 役割別レビュー記録（レビュー担当記入）

最終レビュー対象差分は `1318b19..db34795`（base origin/main `1318b19` / 最終終端 `db34795`）。各担当は最終 HEAD の同一変更セットに対し独立に判定を返却した。実装差分の必須7担当に加え、サイクル3〜4で spec / #16 work-item / confirmation.md の文書是正を含むため参照整合レビュー担当を必須担当として追加した。

### 構造レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 層別責務・依存方向・公開範囲規則に適合。bw-login 結線が domain（summary/command）→ ports（capability 契約）→ application（順序制御）→ adapters（翻訳）→ support/protection（外部処理境界 backend）の境界へ正しく分配されている。固定 secret 意味づけ・業務規則・停止条件を support へ寄せていない。remediation での `Stdio::null()` 追加は protection backend の技術境界内に収まり層責務を移していない。

### 運用整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 実行手順・役割分離・ゲート条件・確認証跡・完了判定ロジックの実運用での強制可能性/監査可能性に具体的懸念なし。FINDING3 の #16 委譲は #16 work-item の構造完了条件・レビュー合格条件と spec 恒久仕様化で構造的に強制され、台帳でも追跡される（サイクル3 で是正済み）。補助記録の exact 同期不足を gate 化していない。

### セキュリティレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 機密情報の露出経路・不正アクセス経路・権限昇格の可能性なし。PR #42 AI レビューが検出した P1（`bw unlock --raw` の `BW_SESSION` stdout 継承漏洩）はサイクル2の `Stdio::null()` 追加で修正済みであり、修正を確認した。master password の protection 借用境界、report の非秘匿出力、失敗時の停止挙動が `security-obligations.md` 制約へ適合。

### 仕様適合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - `work-items/integration.md` の `境界維持の観点`・`構造完了条件`・`完了条件` を現行コードへ直接照合し未解消なし。サイクル2で要修正だった spec との乖離は、サイクル3で spec に `--check bw-login` 現行範囲＝CLI 起動可能性確認＋#16 委譲を恒久仕様化したことで解消し、実装範囲と spec が整合した。

### テストレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 完了条件のテスト検証項目に対するテスト網羅あり（unit 209 / stub integration 42、remediation の stdout 漏洩防止テスト 4 件を含む）。internal backend stub は配置形式ではなく責務基準で adapter 層責務に一致し、production command path を変形せず、test 側は初期/最終 datastore 観測に限定。production source tree への test double 混入なし。

### ドキュメントレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - `application/run_bw_login.rs` の `run_*` entrypoint、core workflow の非自明 internal type/function、port/adapter/support の層責務境界に対する doc comment が what ではなく why/責務境界を説明しており実装と整合。remediation で `login_and_unlock` / OTP / reachability の doc を `Stdio::null()` 実装と一致させた。テストケースへの機械的ヘッダ必須化はしていない。

### アーキテクチャ整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - モジュール全体として一貫した1つの設計を表現しており、責務が層をまたいで一貫分配されている。薄い port/adapter のための業務判断・usecase 手順・固定 secret 意味づけの support 寄せなし。同一 production command path・同一 port 契約・compile-time feature selection を維持。

### 参照整合レビュー担当（文書是正分）

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - サイクル3〜4の文書是正（spec の `--check bw-login` 恒久仕様化、#16 work-item の後続義務追跡、confirmation.md 整合）について、リンク・参照先・ファイルパス・定義の一貫性を確認。サイクル3で発生した行番号参照のずれ（停止条件の参照位置ずれ）は、サイクル4で行番号参照→見出し名参照へ是正し、docs-governance の行番号依存禁止へも適合した。壊れた参照は残っていない。

### 起動不能役割がある場合の記録参照

- 記録参照: `該当なし`（実装差分必須7担当 + 文書是正の参照整合レビュー担当すべて起動・判定返却済み）

## 集約判定

- 集約後レビュー判定: `合格`
- 集約判定要約: `所見なし`
- 最終対象差分: `1318b19..db34795`（base origin/main `1318b19` / 最終終端 `db34795`）
- 集約根拠:
  - 最終対象差分 `1318b19..db34795` に紐づく確認証跡（`./confirmation.md`）が存在し、build(default + stub) / clippy -D warnings / fmt --check / test 209 unit + 42 stub integration / `cargo xtask check`・既存コマンド退行なし・stdout 漏洩防止テスト追加を記録している。
  - サイクル1（`bdb3171`）では必須7担当全員合格だったが、マージ前 AI レビュー（PR #42）が実バグ P1 を後続検出したため差し戻した。以後サイクル2（`b0340d7`、実装修正）・サイクル3（`991245d`、文書是正）・サイクル4（`db34795`、参照是正）の remediation を経て、最終 HEAD で全必須担当が再判定で `合格` に揃った。
  - 実装差分（executable behavior を含む変更）の必須7担当（構造・運用整合・セキュリティ・仕様適合・テスト・ドキュメント・アーキテクチャ整合）が最終 HEAD の同一変更セットに対し全員 `合格`・finding なしを返却した。サイクル3〜4の文書是正を含むため参照整合レビュー担当も追加で `合格`・finding なしを返却した。
  - 主成果物 `実コード差分` の作業項目につき、`work-items/integration.md` の `境界維持の観点` と `構造完了条件` に対し現行コード追跡で未解消項目が残っていないことを仕様適合・構造・アーキテクチャ整合の各担当が確認した。
  - 1件の `不合格`・`要修正` もなく、`finding あり` の記録もないため、集約規則上 `集約後レビュー判定: 合格` が成立する。
- 差戻し事項: `なし`（サイクル1の AI レビュー P1、サイクル2の仕様適合/運用整合 要修正、サイクル3の参照整合 不合格はいずれも後続サイクルで解消済み）
- 後続対応状態: `不要（未解消 finding なし）`
- 懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項が1件でも残る場合は `合格` を記録しない。
- 後続対応メモ: コミット関連作業は別 subagent へ委譲する。完了判定は完了判定担当の責務であり本記録では完了判定しない。`support/protection/bw_login.rs` の実 `bw` CLI に対する end-to-end 検証と真のサービス到達確認（`--check bw-login` の server 到達性確認への拡張）は #16（Bitwarden Password Manager）の責務であり、spec 恒久仕様化と #16 work-item の構造完了条件・レビュー合格条件で構造的に強制される旨は confirmation.md および #16 work-item に記録済み。本統合は internal stub による CLI 経路の結線・順序・停止条件の検証にとどまる。

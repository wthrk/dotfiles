# Bitwarden Password Manager レビュー記録

この文書は `docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Password Manager` に対する固定実装単位 `レビュー` の記録先である。

## 実装担当からの引き継ぎ

- レビュー状態: `完了`
- 対象ブランチ: `feat/secrets-bw-login-issue-16`
- 確認開始時 HEAD: `1318b19`（`main` 起点）
- 対象差分識別子: `main...feat/secrets-bw-login-issue-16`（#16 `bw-login` 実装 + verify-yubikey `--check bw-login` 実体化 + README 文書化）
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

- 確認状態: `完了`
- 確認対象（ファイル/経路）: `application/run_bw_login.rs`、`support/protection/bw_login.rs`、`support/protection.rs`、`adapters/bw/login_adapter.rs`
- 所見: master password（`bw-password`）は `ProtectedSecret` のまま port へ渡り、平文化は `with_master_password` の `with_secret_password_str` borrow closure 内に閉じる。`with_secret_password_str` は `pub(in crate::secrets::support::protection)` で外部層へ非公開。application は平文を取り出さない。`bw-email` も protection 境界内で `BwLoginEmail` へ翻訳。
- 差戻し要否: `否`
- 未実施理由（未実施時のみ）: `該当なし`

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 確認状態: `完了`
- 確認対象（出力経路）: `adapters/bw/login_adapter.rs`、`adapters/bw/login_stub.rs`、`adapters/io/report.rs`、`domain/bw_login.rs`
- 所見: master password は子プロセスの `BW_PASSWORD` env でだけ渡し argv 非搭載、親プロセスの永続環境へ設定しない（`Command::env` は子限定）。login の stdin/stdout/stderr は `Stdio::null()` で破棄、unlock は `--raw` stdout のみ capture、stderr 破棄。エラー本文は固定文字列で secret を含まない。stub observation は email/OTP/unlocked のみで master password を出さない。`BW_SESSION` は surface 専用で disk/dotfile 非永続。
- 差戻し要否: `否`
- 未実施理由（未実施時のみ）: `該当なし`

### 3) 権限境界・永続化・失敗時挙動

- 確認状態: `完了`
- 確認対象（境界/保存先/失敗経路）: `application/run_bw_login.rs`、`application/run_verify_yubikey_with.rs`、`adapters/bw/login_adapter.rs`
- 所見: login/unlock 失敗時は `bail!` で停止し session を返さず report も書かない（unit/integration テストで確認）。verify `--check bw-login` は session を `.map(|_session| ())` で破棄し surface しない。`setrlimit(CORE,0,0)` の core dump 保護は維持。直接 libc 呼び出しなし（std のみ）。
- 差戻し要否: `否`
- 未実施理由（未実施時のみ）: `該当なし`

## 役割別レビュー記録（レビュー担当記入）

### 構造レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 初回レビューで application 層に use-case 独自集約 struct `LoadedVerificationSecrets` の新設を検出し `要修正` 判定（`use case は独自型を定義せず domain 層型のみ` 規則違反）。
  - 是正で当該 struct を除去し、3 つの独立 `Option<ProtectedSecret>` 束縛 + タプル返却（domain 型・標準型のみ）へ置換。再レビュー（新規セッション・全項目再検証）で `合格`。
  - `bw` CLI 起動（`Command::new`）は adapter 境界（`login_adapter.rs`）のみに閉じ、application は順序制御、domain は process 実行詳細に非依存。adapter 新規ファイルに port trait 実装以外の公開シンボルなし。internal backend stub は許可条件を満たす。

### 運用整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - login/unlock 順序・失敗時停止、`verify-yubikey --check bw-login`/`--all` の到達確認、`--all`/`--check` 併用の事前拒否、引数なし時の `skipped` 機械可読状態、`BW_SESSION` の surface 非永続、primary/spare の manual login validation 手順文書化はいずれも強制可能/監査可能。
  - 初回の唯一の所見は「#16 差分が未コミットで対象差分識別子が未固定」というプロセス指摘であり、本コミットで対象差分（`main...feat/secrets-bw-login-issue-16`）を固定して解消。

### セキュリティレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 上記「セキュリティ所見記録」1〜3 のとおり、秘密値の borrow 境界限定、`BW_PASSWORD` env の子プロセス限定・非永続、漏えい経路なし、失敗時の session 非返却、直接 libc 不使用を確認。

### 仕様適合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - spec L84/L86/L107/L155/L176-178/L190/L201 の挙動（YubiKey からの `bw-email`/`bw-password` 取得 → OTP → `bw login --method 3` → `bw unlock --raw`、`bw` CLI の login/unlock 限定、`--email` override、`BW_PASSWORD` 非保存・`BW_SESSION` 非永続、verify の外部確認と `skipped`、失敗条件）を実装が満たす。仕様を踏み越えた独自前提なし。

### テストレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - login/unlock 順序（`mockall::Sequence`）、`--email` override で `bw-email` 非読み出し、master password が port へ保護値として渡る、login 失敗時の report 非記述・停止、verify `--check bw-login`/`--all`/引数なし `skipped`、observation に master password 非出力をテストで網羅。
  - 残留メモ（差戻し対象外）: real adapter の argv/env 構築の直接 unit test はなく、integration stub が login+unlock を 1 遷移へ集約するため argv/env 配線は直接検証されない。構造的に境界内に閉じた翻訳詳細であり完了条件（spec 挙動）の網羅には影響しない。

### ドキュメントレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - README の `### Bitwarden Password Manager login` 節と `#### primary / spare の manual login validation` 小節が canonical 仕様（spec L82-86/L107/L155/L178/L190）と整合し、#16 完了条件「primary と spare のどちらでも manual login validation を行える手順を文書化する」を満たす。記載コマンド・フラグ（`--serial`/`--email` のみ）は実装と一致。canonical spec は改変せず docs-governance の配置・重複禁止に適合。

### アーキテクチャ整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 新規経路（`domain/bw_login.rs` → `ports/bw.rs`・`ports/io.rs` → `application/run_bw_login.rs` → `adapters/bw/login_adapter.rs` → `support/protection/bw_login.rs`）が既存 use case と同一の層分配パターンを踏襲。`bw` CLI 依存は adapter 境界へ閉じ、port capability 粒度も適切。現行アーキテクチャ固定の前提を崩す大幅な作り替えなし。

### 参照整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - doc コメント・README が引用する spec 行番号（L84/L86/L107/L155/L178/L190/L201）が正本内容と一致。参照シンボル（`SecretName::BwEmail/BwPassword`、`with_secret_password_str`、`to_test_bytes/from_test_bytes`、`BW_LOGIN_STUB_SPEC_ENV` 等）・コマンド名・フラグ名・secret 名がすべて実体に解決。誤引用・存在しない参照なし。

### 起動不能役割がある場合の記録参照

- 記録参照: `該当なし（全 8 役割を fresh subagent で実施）`

## 集約判定

- 集約後レビュー判定: `合格`
- 集約判定要約: `所見なし`
- 集約根拠:
  - 初回ゲートで構造レビューが application 層の use-case 独自 struct `LoadedVerificationSecrets` を検出し `要修正`。是正（struct 除去 → domain 型 + Option/タプル）後、構造レビューを新規セッションで再実施し `合格`。
  - 運用整合の初回所見（対象差分未コミット）は本コミットで対象差分範囲を固定して解消。
  - 他 6 役割（セキュリティ/仕様適合/テスト/ドキュメント/アーキテクチャ整合/参照整合）は初回から `合格`。
  - リポジトリ CI ゲート `cargo xtask check static`（fmt / `RUSTFLAGS=-D warnings cargo check` / clippy `-D warnings` / `cargo test --workspace --all-targets` / stub feature 統合テスト / `secrets::application` 単体テスト / nix 静的検査）は是正後も `all checks passed!` で合格。
- 差戻し事項: `なし（是正完了）`
- 後続対応状態: `完了`
- 懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項が1件でも残る場合は `合格` を記録しない。
- 後続対応メモ: `テストレビューの残留メモ（adapter argv/env の直接テスト不在）は完了条件外の翻訳詳細であり差戻し対象外。`

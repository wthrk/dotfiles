# GnuPG / SSH レビュー記録

この文書は `docs/tasks/secret-recovery/tasks.md` の作業項目 `GnuPG / SSH` に対する固定実装単位 `レビュー` の記録先である。

## 実装担当からの引き継ぎ

- レビュー状態: `未着手`
- 対象ブランチ: `未記入`
- 確認開始時 HEAD: `未記入`
- 対象差分識別子: `未記入`
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

- 確認状態: `未着手`
- 確認対象（ファイル/経路）: `未記入`
- 所見: `未記入`
- 差戻し要否: `未記入`
- 未実施理由（未実施時のみ）: `未記入`

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 確認状態: `未着手`
- 確認対象（出力経路）: `未記入`
- 所見: `未記入`
- 差戻し要否: `未記入`
- 未実施理由（未実施時のみ）: `未記入`

### 3) 権限境界・永続化・失敗時挙動

- 確認状態: `未着手`
- 確認対象（境界/保存先/失敗経路）: `未記入`
- 所見: `未記入`
- 差戻し要否: `未記入`
- 未実施理由（未実施時のみ）: `未記入`

## 役割別レビュー記録（レビュー担当記入）

### 構造レビュー担当

- 判定: `<合格|要修正|不合格>`
- 判定要約: `<所見なし|主要論点要約>`
- 根拠:
  - `未記入`

### 運用整合レビュー担当

- 判定: `<合格|要修正|不合格>`
- 判定要約: `<所見なし|主要論点要約>`
- 根拠:
  - `未記入`

### セキュリティレビュー担当

- 判定: `<合格|要修正|不合格>`
- 判定要約: `<所見なし|主要論点要約>`
- 根拠:
  - `未記入`

### 仕様適合レビュー担当

- 判定: `<合格|要修正|不合格>`
- 判定要約: `<所見なし|主要論点要約>`
- 根拠:
  - `未記入`

### 参照整合レビュー担当

- 判定: `<合格|要修正|不合格>`
- 判定要約: `<所見なし|主要論点要約>`
- 根拠:
  - `未記入`

### 起動不能役割がある場合の記録参照

- 記録参照: `未記入`

## 集約判定

- 集約後レビュー判定: `<合格|要修正|不合格>`
- 集約判定要約: `<所見なし|主要論点要約>`
- 集約根拠:
  - `未記入`
- 差戻し事項: `未記入`
- 後続対応状態: `未着手`
- 懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項が1件でも残る場合は `合格` を記録しない。
- 後続対応メモ: `レビュー未開始のため未整理`

## 現行サイクル（2026-05-31 / 増分1: encrypted envelope domain 層）

- GitHub: 実装サブ issue #36（親 #14）
- 対象ブランチ: `feat/secrets-restore-gpg-issue-14`（origin/main ベース）
- 確認開始時 HEAD: `ca7c9ca`（直前は現行アーキ固定の文書是正コミット）
- 対象差分識別子: 増分1 = `rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs`（新規）＋ `rust/dotfiles-cli/src/secrets/domain.rs`（`pub mod gpg_backup;` 追加）。`git --no-pager diff HEAD` ＋ 新規 untracked ファイル。
- スコープ: encrypted envelope の domain 層のみ（型・JSON (de)serialize・schema 検証・fingerprint 正規化・recipient 照合・`exported_at` UTC RFC3339 検証・単体テスト）。port/adapter（gpgme/YubiKey/BWS）・application・command は後続増分。
- 実検証: `cargo test -p dotfiles-cli --lib secrets::domain::gpg_backup` = 22 passed / 0 failed（親オーケストレーターが独立実行で確認）。`clippy`/`fmt --check`（`-D warnings`）通過。

### サイクル1（差し戻し）

- 構造 / アーキテクチャ整合 / セキュリティ / テスト / ドキュメント / 運用整合: `判定: 合格`
- 仕様適合レビュー担当: `判定: 要修正` / 判定要約: `metadata.exported_at` が設計「決定事項」の UTC RFC3339 形式として未検証（非空のみ）、doc は RFC3339 と表明。
- 集約後レビュー判定: `要修正` → `S1` 差し戻し。
- 是正: `validate_rfc3339_utc`（形式・数値範囲・UTC offset 検証）を `EnvelopeMetadata::from_wire` に配線、異常系/正常系テスト6件追加、doc を実装保証内容へ整合。

### サイクル2（再レビュー / 是正後）

- 構造レビュー担当: `判定: 合格` / 所見なし
- アーキテクチャ整合レビュー担当: `判定: 合格` / 所見なし
- セキュリティレビュー担当: `判定: 合格` / 所見なし
- 仕様適合レビュー担当: `判定: 合格` / 所見なし（前サイクル finding 解消を直接照合で確認）
- テストレビュー担当: `判定: 合格` / 所見なし
- ドキュメントレビュー担当: `判定: 合格` / 所見なし
- 運用整合レビュー担当: `判定: 合格` / 所見なし
- 集約後レビュー判定: `合格`
- 集約判定要約: 所見なし
- 集約根拠: 実コード差分の必須7レビュー担当が再レビューで全員合格。envelope schema 全項目（version/metadata/recipients/ciphertext・固定値・byte長・fingerprint 正規化・recipient 両一致照合・exported_at UTC RFC3339）を設計へ直接照合し未解消なし。domain は鍵リング/process I/O 非依存（構造完了条件）。secret 露出経路・test double 混入なし。各担当は対象コードを独立に直接読んで判定。
- 後続対応状態: コミット着手可（増分1）。後続増分（port/adapter/application/command/Home Manager/zsh）は別サイクルで継続。

### サイクル3（PR #37 Codex Review 指摘の是正）

- 契機: PR #37（コミット `67f11e0` 時点）に対する GitHub Codex Review bot の inline 指摘5件（いずれも P2、重複除外後）。
- 是正対象差分: `rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs` ＋ `docs/secret-recovery/gnupg-ssh-design.md`（piv_slot 型1行）。
- 是正した指摘:
  1. `base64_decode` の padding を最終 chunk 末尾位置に限定（`AA==AAAA` 等の非末尾 padding を停止）。
  2. `exported_at` の暦日妥当性検証（`days_in_month` + 閏年判定で `2026-02-31`/平年 `2025-02-29` 等を停止）。
  3. wire 構造体4種へ `#[serde(deny_unknown_fields)]`（未知 field を停止）。
  4. `piv_slot` を正本 `bitwarden-secrets-manager-design.md:93` の文字列 `"82"` 固定へ統一（wire `String` 化・`to_json` 文字列・`gnupg-ssh-design.md:28` を string 明示で docs 整合）。
  5. テスト fixture の nonce/tag を一意 byte 列化し `String::replace` の取り違えを解消。
- 実検証: `cargo test -p dotfiles-cli --lib secrets::domain::gpg_backup` = 30 passed / 0 failed（親オーケストレーター独立実行で確認）。`clippy`/`fmt --check`（`-D warnings`）通過。
- 必須7レビュー担当（構造・アーキテクチャ整合・セキュリティ・仕様適合・テスト・ドキュメント・運用整合）: 全員 `判定: 合格` / 所見なし。
- 集約後レビュー判定: `合格`
- 集約根拠: 是正5件すべて非空虚なテストで検証され、設計（envelope schema・piv_slot 文字列正本・base64 妥当性・暦日 RFC3339・未知 field 拒否）へ直接照合し未解消なし。domain 純粋性・secret 非露出維持。各担当が対象コードを独立に直接読んで判定。
- 後続対応状態: コミット・push 後に PR #37 レビュースレッドへ回答・resolve。

### サイクル4（PR #37 Codex 再レビュー指摘の是正）

- 契機: push（`df83d42`/`e5dc241`）後の Codex 再レビュー（コミット `e5dc24`）で新規指摘1件（id 3330152377、line 587）。
- 指摘: `base64_decode` が padding で捨てる sextet の下位 bit が 0 か検証せず、非 canonical な base64（`AB==`/`AAB=`）を受理する。
- 是正対象差分: `rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs`（`base64_decode` のみ、+35行）。
- 是正: RFC 4648 §3.5 の canonical 性検証を追加（2 padding=2番目 sextet 下位4bit、1 padding=3番目 sextet 下位2bit が非0なら `invalid_base64` で停止）。既存の padding 位置・最終chunk限定検証は退行なし。
- 実検証: `cargo test -p dotfiles-cli --lib secrets::domain::gpg_backup` = 31 passed / 0 failed（親独立実行で確認）。`clippy`/`fmt`（`-D warnings`）通過。
- 必須7レビュー担当: 全員 `判定: 合格` / 所見なし（テスト担当は mutation test で新規テストの非空虚性を実証）。
- 集約後レビュー判定: `合格`
- 集約根拠: canonical 検証の bit 算術が RFC 4648 §3.5 と一致し、非 canonical 拒否・canonical 受理を非空虚テストで網羅。既存 envelope schema 検証の退行なし。domain 純粋性・secret 非露出維持。
- 後続対応状態: コミット・push 後に PR #37 の新規スレッド（id 3330152377）へ回答・resolve。

### サイクル5（PR #37 Codex 再レビュー指摘の是正）

- 契機: `@codex review` で `3de62a7` を明示再レビューし、新規指摘2件（id 3330184006 / 3330184007）。
- 指摘:
  - (A) `exported_at` の秒60を位置無制限で受理（RFC3339 §5.7 は leap second 60 を UTC 月末 23:59:60 に限定）。
  - (B) wire（保存値）の fingerprint をユーザー入力向け正規化 parser に通すため非 canonical（uppercase/区切り/空白）を受理し、`to_json` で書き換えて破損を隠す。設計上は保存値が既に canonical（lowercase hex・区切りなし）であるべき。
- 是正対象差分: `rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs`（+187/-12）。
- 是正:
  - (A) 秒60を `hour==23 && minute==59 && day==days_in_month(year,month)`（UTC 月末日 23:59:60）のときのみ許可、他位置の60と≥61を停止。
  - (B) wire 保存値 fingerprint 用の厳格 canonical 検証 `validate_canonical_wire_fingerprint`（正規化せず非 canonical を停止）を追加し `*::from_wire` で使用。runtime 照合入力（`ConnectedYubiKey`）は従来の `normalize_fingerprint` を維持し、二経路を doc で分離。`to_json` は保存値を無変換出力。
- 実検証: `cargo test -p dotfiles-cli --lib secrets::domain::gpg_backup` = 35 passed / 0 failed（親独立実行で確認）。`clippy`/`fmt`（`-D warnings`）通過。
- 必須7レビュー担当: 全員 `判定: 合格` / 所見なし（テスト担当は新規テストの非空虚性を確認）。
- 集約後レビュー判定: `合格`
- 集約根拠: (A)(B) とも設計（exported_at UTC RFC3339・fingerprint は canonical lowercase hex 保存値）へ整合、非空虚テストで網羅、既存 schema 検証の退行なし。wire厳格×runtime正規化の照合一致を確認。malformed 保存値の停止が後続 restore/verify の前提を強化。
- 後続対応状態: コミット・push 後に PR #37 の新規スレッド（id 3330184006 / 3330184007）へ回答・resolve。

### サイクル6（PR #37 Codex 再レビュー指摘の是正）

- 契機: `@codex review`（head `6108173`）で新規指摘1件（id 3330204008）。
- 指摘: 秒60を任意の月末日（例 `2026-05-31T23:59:60Z`）で受理する。RFC3339 §5.7 は秒60を leap second が発生する月末に限るとし、許可月（June/December 等）まで絞るべき。
- 判断（実施・確定的厳格化）: bot 提案の「June/December 限定」は RFC3339 上不正確（leap second は原理上どの月末にも起こり得る）。完全な検証には可変な leap-second テーブルが必要で domain 検証に不適切。`exported_at` は本ツールが export 時刻に生成する wall-clock UTC timestamp であり leap second は正当に発生しない。→ **秒60を一律拒否（秒 0..=59 のみ許可）** する確定的厳格化を採用。
- 是正対象差分: `rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs`（+21/-25）。前サイクルの月末 23:59:60 許可分岐を除去し `if second > 59 { Err }` に。`days_in_month` は暦日妥当性検証で継続使用。
- 実検証: `cargo test -p dotfiles-cli --lib secrets::domain::gpg_backup` = 35 passed / 0 failed（親独立実行で確認）。`clippy`/`fmt`（`-D warnings`）通過。
- 必須7レビュー担当: 全員 `判定: 合格` / 所見なし。
- 集約後レビュー判定: `合格`
- 集約根拠: 秒60拒否は設計（exported_at UTC RFC3339）と矛盾せず受理域を縮小するのみ、非空虚テストで網羅、暦日/offset/fraction/他 schema 検証の退行なし。停止条件は識別可能な domain error。
- 後続対応状態: コミット・push 後に PR #37 の新規スレッド（id 3330204008）へ判断根拠付きで回答・resolve。

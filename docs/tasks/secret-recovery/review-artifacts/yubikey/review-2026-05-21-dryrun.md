# YubiKey レビュー記録

この文書は [tasks.md](../../tasks.md#新規マシン秘密情報復旧基盤タスク) の作業項目 `YubiKey` に対する固定実装単位 `レビュー` の記録先である。

## 実装担当からの引き継ぎ

- レビュー状態: `未着手`（`コード差分なし` のため合否判定を開始しない）
- 対象ブランチ: `feat/yubikey-secret-storage`
- 確認開始時 HEAD: `7b08f6c`
- 対象差分識別子: `コード差分なし`
- 実装側確認証跡: [confirmation-2026-05-21-dryrun.md](confirmation-2026-05-21-dryrun.md#yubikey-確認記録)
- 実装側で実施済みの確認:
  - `direnv exec . git diff --check`（空出力）

## レビュー担当チェック項目

1. `YubiKey` 作業項目のスコープ外へ越境していないこと。
2. `docs/secret-recovery/secret-recovery-spec.md` と `docs/secret-recovery/yubikey-secret-storage-design.md` の語彙・責務境界・参照整合が維持されていること。
3. 責務境界と依存方向が [レビュー観点チェックリスト](../../../../architecture/review-checklist.md#レビュー観点チェックリスト構造) の観点に適合していること。
4. [yubikey.md](../../work-items/yubikey.md#12-yubikey-秘密情報保存) の `レビュー合格条件` を満たすこと。
5. 仕様・設計・作業定義文書の要求挙動、停止条件、成功条件が実装に反映されていること。

## セキュリティ所見記録（必須）

### 1) 秘密値・認証情報の扱い

- 判定: `未着手（コード差分なし）`
- 確認対象（ファイル/経路）: `rust/dotfiles-cli/src/secrets/**`
- 所見: `実装差分が存在しないため判定保留`
- 差戻し要否: `判定保留`
- 未実施理由（未実施時のみ）: `対象コード差分なし`

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 判定: `未着手（コード差分なし）`
- 確認対象（出力経路）: `CLI 標準出力・ログ・プロセス引数・一時ファイル`
- 所見: `実装差分が存在しないため判定保留`
- 差戻し要否: `判定保留`
- 未実施理由（未実施時のみ）: `対象コード差分なし`

### 3) 権限境界・永続化・失敗時挙動

- 判定: `未着手（コード差分なし）`
- 確認対象（境界/保存先/失敗経路）: `adapter 境界・保存経路・エラー処理経路`
- 所見: `実装差分が存在しないため判定保留`
- 差戻し要否: `判定保留`
- 未実施理由（未実施時のみ）: `対象コード差分なし`

## 役割別レビュー判定（レビュー担当記入）

- 構造レビュー担当 判定: `未着手（コード差分なし）`
- 構造レビュー担当 agent/run 識別子: `agent:review-structure-yubikey / run:2026-05-21-yubikey-rs-001`
- 運用整合レビュー担当 判定: `未着手（コード差分なし）`
- 運用整合レビュー担当 agent/run 識別子: `agent:review-ops-yubikey / run:2026-05-21-yubikey-ro-001`
- セキュリティレビュー担当 判定: `未着手（コード差分なし）`
- セキュリティレビュー担当 agent/run 識別子: `agent:review-security-yubikey / run:2026-05-21-yubikey-rsec-001`
- 仕様適合レビュー担当 判定: `未着手（コード差分なし）`
- 仕様適合レビュー担当 agent/run 識別子: `agent:review-spec-yubikey / run:2026-05-21-yubikey-rsp-001`
- 参照整合レビュー担当 判定: `未着手（コード差分なし）`
- 参照整合レビュー担当 agent/run 識別子: `agent:review-reference-fallback-yubikey / run:2026-05-21-yubikey-rr-fallback-001`
## 役割別フォールバック記録（必要時のみ必須）

- フォールバック記録の記入規則: [implementation-guidelines.md#planning--implementation--review-の役割分担](../../../../secret-recovery/implementation-guidelines.md#planning--implementation--review-の役割分担)
- no-reuse の要件: [implementation-guidelines.md#planning--implementation--review-の役割分担](../../../../secret-recovery/implementation-guidelines.md#planning--implementation--review-の役割分担)

- 対象役割: `参照整合レビュー担当`
- 対象役割の agent/run 記録: `agent:review-reference-yubikey / run:launch-failed-2026-05-21-yubikey-rr-001`
- 起動失敗理由: `専用 reviewer 起動 API が利用不可`
- 起動失敗証跡: `2026-05-21T14:36+09:00 reviewer launcher error: role not available`
- 代替実行者: `fallback-reviewer-reference-yubikey`
- 代替実行者 agent/run 識別子: `agent:review-reference-fallback-yubikey / run:2026-05-21-yubikey-rr-fallback-001`
- no-reuse 規則充足根拠: `代替実行者は他レビュー役割と異なる agent 識別子で単独実行`

## 集約判定（進捗判定担当のみ記入）

- 集約後レビュー判定: `未着手（コード差分なし）`
- 差戻し事項: `なし（判定未開始）`
- 後続対応判定: `未着手（レビュー未開始のため判定保留）`
- 進捗判定担当 agent/run 識別子: `agent:progress-yubikey / run:2026-05-21-yubikey-pj-001`
- 進捗判定担当が不在時の代替実行有無: `なし`
- 代替実行時フォールバック記録参照: `該当なし`

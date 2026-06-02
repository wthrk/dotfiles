# #11 系粗粒度進捗

この文書は、`#11 新規マシン秘密情報復旧基盤を実装する` を親 issue とした粗粒度進捗を保持する。

## 親 issue

- `#11 新規マシン秘密情報復旧基盤を実装する`
- 完了条件:
  - `#12` から `#17` がすべて閉じていること。
  - `#17 新規マシン復旧フロー統合` が完了していること。

## 履歴ソース（旧パス）

以下は履歴参照専用の旧パスであり、現行正本パスではない。

- `git show 1face6c:docs/secret-recovery/tasks.md`
- `git show 1edc4d9:docs/secret-recovery/tasks.md`
- `git show e91c8f2:docs/secret-recovery/yubikey-secret-storage-design.md`

## 段階モデル

- `Design PR`
- `Design review`
- `Implementation PR`
- `Code review`
- `Validation`
- `Done`

出典: `git show 1face6c:docs/secret-recovery/tasks.md`（履歴参照専用）

## milestone 復元

- First PR:
  - PR: `#19`
  - 内容: documentation-only の全体設計とタスク構造追加
  - 状態: `merge 済み`
  - 検証: `direnv exec . cargo xtask check static`
  - 出典:
    - `git show 1face6c:docs/secret-recovery/tasks.md`
    - commit `1edc4d9 docs: 新規マシン秘密情報復旧基盤の設計を追加 (#19)`
- YubiKey design PR:
  - 対象 issue: `#12`
  - PR: `#21`
  - 状態: `merge 済み`
  - 出典:
    - commit `e91c8f2 docs(secret-recovery): YubiKey秘密情報保存設計を追加 (#21)`
    - `git show e91c8f2:docs/secret-recovery/yubikey-secret-storage-design.md`

## sub-issue 一覧

| Issue | 名称 | 現在状態 | 根拠 |
| --- | --- | --- | --- |
| `#12` | YubiKey 秘密情報保存 | `完了` | `#21` merge、現行 tasks で `完了`、review 正本は `review-artifacts/yubikey/review.md` |
| `#13` | Bitwarden Secrets Manager クライアント | `進行中（Mendel再レビュー Fail 対応中）` | root/area ledger は進行中へ更新、`review-artifacts/bitwarden-secrets-manager/review.md` は `集約後レビュー判定: 要修正`、未解決 thread のローカル是正を継続 |
| `#14` | GPG 復元 / gpg-agent SSH 対応 | `未着手（履歴上の追加進捗未検出）` | 旧 tasks の issue 定義のみ復元 |
| `#15` | password-store 復元 | `未着手（履歴上の追加進捗未検出）` | 旧 tasks の issue 定義のみ復元 |
| `#16` | Bitwarden Password Manager CLI ログイン | `未着手（履歴上の追加進捗未検出）` | 旧 tasks の issue 定義のみ復元 |
| `#17` | 新規マシン復旧フロー統合 | `実装済み（現行サイクル集約レビュー合格）` | root/area ledger を `実装済み（現行サイクル集約レビュー合格）` へ更新。`review-artifacts/integration/review.md` は必須7担当全員合格・`集約後レビュー判定: 合格`（現行サイクル差分 `1318b19..bdb3171`）。完了判定は完了判定担当の責務 |
| `#18` | 最終ドキュメント整理 | `移管済み（repo-global へ移設）` | `docs/tasks/repo-governance/tasks.md`、`docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md`、`docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md` を参照 |

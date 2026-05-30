# Bitwarden Secrets Manager PR #35 Copilot 指摘対応確認

## 対象差分

- 作業 branch: `fix/pr-35-copilot-review`
- PR: `https://github.com/wthrk/dotfiles/pull/35`
- 対象: Copilot review comments `3328346236`, `3328346238`, `3328346241`, `3328366503`, `3328366504`, `3328366505`, `3328366506`, `3328366509`, `3328366511`

## 対応内容

- `gpg-secret-key-backup` envelope schema に AES-GCM `tag` の独立 field と長さを固定した。
- recipient の PIV slot を既存 YubiKey 設計と同じ `82` に同期し、`public_key_fingerprint` を SPKI DER SHA-256 lowercase hex として固定した。
- backup export 入力を `gpgme` in-memory export とし、export bytes の再解析、fingerprint 一致、argv / shell history / ログ / 永続一時ファイル禁止を明文化した。
- BWS 更新の stale overwrite 防止条件を revision / updatedAt / ETag 相当、または exact secret value bytes の SHA-256 digest に変更した。
- 復号済み backup から導出した primary fingerprint と envelope `metadata.primary_fingerprint` の一致検証を restore / BWS / spec の各境界へ同期した。
- `dotfiles secrets restore-gpg` の手順順序を「envelope 検証 → unwrap/復号 → primary fingerprint 導出と `metadata.primary_fingerprint` 一致検証 → import」に修正し、関連正本（`gnupg-ssh-design.md` / `secret-recovery-spec.md`）と整列させた。
- spare recipient 追加を含む envelope 更新を stale overwrite 防止の必須対象に拡張し、revision / updatedAt / ETag 相当または exact value bytes digest による照合を必須化した。
- `metadata.primary_fingerprint` 形式を lowercase hex 40 文字・区切りなしに固定し、GnuPG / BWS / spec の各文書で同期した。
- `verify-yubikey --check bws` の確認契約を 2 secret 取得可否のみから、`gpg-secret-key-backup` envelope schema 検証、接続中 YubiKey recipient 照合、unwrap なしでの復旧可能性確認まで拡張した。

## 確認

- `git diff --check`
  - 結果: 成功
- `rg -n 'version / metadata / ciphertext / recipients' /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/bitwarden-secrets-manager-design.md /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/gnupg-ssh-design.md /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/secret-recovery-spec.md`
  - 結果: no-hit（終了コード 1）
- `rg -n 'version / metadata / recipients / ciphertext' /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/bitwarden-secrets-manager-design.md /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/gnupg-ssh-design.md /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/secret-recovery-spec.md`
  - 実出力:
    - `/Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/gnupg-ssh-design.md:62:2. BWS から取得した `gpg-secret-key-backup` encrypted envelope をメモリ上で検証する（version / metadata / recipients / ciphertext）。`
    - `/Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/gnupg-ssh-design.md:91:3. 取得値の envelope 形式（version / metadata / recipients / ciphertext）を検証し、接続中 YubiKey と一致する recipient がない場合は停止する。`
    - `/Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/gnupg-ssh-design.md:175:- backup envelope の形式検証（version / metadata / recipients / ciphertext）に失敗する。`
    - `/Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/secret-recovery-spec.md:161:3. envelope 形式（version / metadata / recipients / ciphertext）を検証し、接続中 YubiKey と一致する recipient が存在しない場合は停止する。`
    - `/Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/secret-recovery-spec.md:202:- `gpg-secret-key-backup` の envelope 形式検証（version / metadata / recipients / ciphertext）に失敗する。`
- `rg -n 'top-level:|top-level は' /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/bitwarden-secrets-manager-design.md /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/gnupg-ssh-design.md /Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/secret-recovery-spec.md`
  - 実出力:
    - `/Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/bitwarden-secrets-manager-design.md:82:- top-level: `version`（number, `1` 固定）/ `metadata` / `recipients` / `ciphertext``
    - `/Users/ya/works/dotfiles/.worktrees/pr-35-copilot-review-fix/docs/secret-recovery/gnupg-ssh-design.md:25:- encrypted envelope は UTF-8 JSON で保存し、`version: 1` を固定する。top-level は `version` / `metadata` / `recipients` / `ciphertext` の 4 要素とする。`

## 未実施

- Markdown-only の設計文書差分のため、`cargo xtask check` は実行していない。
- fresh review / 集約 / commit gate 前のため、PR review comment への reply / resolve は実施していない。

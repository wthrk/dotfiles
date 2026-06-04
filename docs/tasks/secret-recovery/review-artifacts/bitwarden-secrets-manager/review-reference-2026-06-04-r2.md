# Bitwarden Secrets Manager 参照整合レビュー 2026-06-04 r2

- レビュー担当: 参照整合レビュー担当（文書是正専用）
- 対象 repo: `/Users/ya/works/dotfiles`
- 対象ブランチ: `fix/bws-provisioning-inputs-issue-44`
- 対象 worktree: current worktree（未コミット差分を含む）
- Active work item: `docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`
- 主確認対象: `README.md`、`docs/secret-recovery/README.md`、`docs/secret-recovery/secret-recovery-spec.md`、`docs/secret-recovery/bitwarden-secrets-manager-design.md`、`docs/secret-recovery/gnupg-ssh-design.md`、`docs/secret-recovery/initial-provisioning-runbook.md`、`scripts/provision-secret-recovery-source.sh`、関連 CLI 定義

判定: 合格
判定要約: 所見なし
根拠:
- 前回 finding の `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` は解消済み。`README.md`、`docs/secret-recovery/secret-recovery-spec.md`、`docs/secret-recovery/gnupg-ssh-design.md`、`docs/secret-recovery/initial-provisioning-runbook.md`、`scripts/provision-secret-recovery-source.sh` の実行手順・契約・script 呼び出しは、CLI 定義の必須 `--primary-fingerprint` option と整合している。
- project gate は、Bitwarden Secrets Manager 側で project `dotfiles-secret-recovery` を手動作成し、`dotfiles` / `scripts/provision-secret-recovery-source.sh` は project を作成せず、登録・更新用 token から同名 project が 1 件だけ見える状態を provisioning 前 gate とする記述に揃っている。
- secret 名は `bw-email`、`bw-password`、`bws-access-token`、`gpg-secret-key-backup`、`password-store-remote`、project 名は `dotfiles-secret-recovery` で、README/spec/design/runbook/script/CLI 定義間で不一致は確認されなかった。
- command 名は `dotfiles secrets restore-gpg`、`dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>`、`dotfiles secrets restore-pass`、`dotfiles secrets bw-login`、`dotfiles secrets gpg-backup register`、`dotfiles secrets gpg-backup add-spare`、`dotfiles secrets pass-remote register`、`dotfiles secrets yubikey rotate-bws-token`、`dotfiles secrets verify-yubikey --check bws` / `--check bw-login` / `--all` が、利用者向け文書と CLI 定義で整合している。
- `dotfiles-secret-recovery-reader`、旧 BWS `machine account` / `service account` 前提の user-facing 記述は確認対象に残っていない。`organization` は `password-store-remote` の GitHub owner 説明、および SDK 内部 field `organization_id` を adapter-local request scope として翻訳する実装コメントに限られ、利用者に Bitwarden organization 入力を要求する前提ではない。
- 対象文書内の相対 Markdown link / anchor は、GitHub 互換の heading slug で解決確認済み。確認対象には `secret-recovery-spec.md`、`bitwarden-secrets-manager-design.md#初期登録手順`、`yubikey-secret-storage-design.md`、`gnupg-ssh-design.md`、`secret-handling.md#secret-handling-policy`、`../architecture/hexagonal-implementation-rules.md#internal-backend-stub-の配置`、`docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`、`scripts/provision-secret-recovery-source.sh` を含め、current worktree 上で参照先の存在または該当見出しを確認した。
- `docs/docs-governance.md` の配置・正本・重複禁止規則に反する二重正本化は確認されなかった。README は導線、spec/design/runbook はそれぞれ仕様・設計・実行手順として役割が分かれており、補助記録の exact 同期を gate 化する記述も確認されなかった。

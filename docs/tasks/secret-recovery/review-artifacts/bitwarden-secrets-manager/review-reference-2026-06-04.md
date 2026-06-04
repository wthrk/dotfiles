# Bitwarden Secrets Manager 参照整合レビュー 2026-06-04

- レビュー担当: 参照整合レビュー担当（文書是正専用）
- 対象 repo: `/Users/ya/works/dotfiles`
- 対象ブランチ: `fix/bws-provisioning-inputs-issue-44`
- 対象 worktree: current worktree（未コミット差分を含む）
- Active work item: `docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`
- 主確認対象: `README.md`、`docs/secret-recovery/README.md`、`docs/secret-recovery/secret-recovery-spec.md`、`docs/secret-recovery/bitwarden-secrets-manager-design.md`、`docs/secret-recovery/initial-provisioning-runbook.md`

判定: 不合格
判定要約: `dotfiles gpg export-ssh-public-key` の user-facing 手順が現行 CLI 必須 option と不整合であり、実行可能なコマンド参照として成立しない。
根拠:
- `rust/dotfiles-cli/src/secrets.rs` では `GpgExportSshPublicKeyOptions.primary_fingerprint` が `#[arg(long)]` の必須 option として定義されているため、利用者向けの実行手順は `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` を示す必要がある。
- `README.md` は `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` と記載しており、現行 CLI 定義と整合している。
- `docs/secret-recovery/initial-provisioning-runbook.md` の対応表、Phase A 手順5、Phase B 手順4は `dotfiles gpg export-ssh-public-key` のみを [CMD] として示しており、必須 `--primary-fingerprint` が欠落している。これは runbook の実行手順として README と CLI 定義に矛盾する。
- `docs/secret-recovery/secret-recovery-spec.md` の到達仕様復旧フロー手順7も `dotfiles gpg export-ssh-public-key` のみを手順として示しており、同じ必須 option 欠落がある。コマンド節見出しとしての省略なら許容できるが、手順行は実行コマンドとして読まれるため README/runbook と不整合である。
- `docs/secret-recovery/gnupg-ssh-design.md` の `dotfiles gpg export-ssh-public-key` 節も入力である primary fingerprint を説明しておらず、spec/runbook の省略を補完できていない。結果として README、spec、design、runbook 間で同一 command の利用契約が一致していない。
- `docs/secret-recovery/initial-provisioning-runbook.md` の Phase B 見出しにある `spec「到達仕様の復旧フロー」L102-112` は、現行 `docs/secret-recovery/secret-recovery-spec.md` の同節が見出し L100、手順 L102-L110、次節見出し L112 であるため、手順番号・行範囲のずれとしては blocker にしない。
- `dotfiles-secret-recovery-reader`、`machine account`、`service account`、Bitwarden organization 前提の user-facing 記述は、確認した `README.md`、`docs/secret-recovery/README.md`、`secret-recovery-spec.md`、`bitwarden-secrets-manager-design.md`、`initial-provisioning-runbook.md` には残っていない。`organization` は `password-store-remote` の GitHub owner 名説明にだけ残っており、本件の旧 BWS machine account 前提ではない。
- 参照リンク・ファイルパスについて、確認対象文書内の相対リンク先である `secret-recovery-spec.md`、`bitwarden-secrets-manager-design.md#初期登録手順`、`yubikey-secret-storage-design.md`、`gnupg-ssh-design.md`、`secret-handling.md#secret-handling-policy`、`../architecture/hexagonal-implementation-rules.md#internal-backend-stub-の配置`、`docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`、`scripts/provision-secret-recovery-source.sh` は current worktree 上で存在または該当見出しが確認できる。
- Secret 名は `bw-email`、`bw-password`、`bws-access-token`、`gpg-secret-key-backup`、`password-store-remote`、project 名は `dotfiles-secret-recovery` で、README/spec/design/runbook 間の定義名は概ね揃っている。ただし上記 command option 欠落により、参照整合レビューとしては合格にできない。

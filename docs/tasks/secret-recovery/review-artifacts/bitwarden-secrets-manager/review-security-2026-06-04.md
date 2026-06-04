# Bitwarden Secrets Manager security review 2026-06-04

判定: 要修正
判定要約: BWS token の平文 argv/env/xtrace 漏えいは実装上確認されなかったが、provisioning script の private repo 所在出力と、BWS 登録用 token / YubiKey 保存用 token の分離契約に反する Rust 側コメントが残る。
根拠:
- 対象: current worktree on `fix/bws-provisioning-inputs-issue-44`。未コミット差分を含む `git status --short --branch` / `git diff --name-status HEAD` で確認した。
- 必須参照: `docs/task-governance/security-obligations.md`、`docs/task-governance/implementation-review-judgement.md`、`docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`、`docs/secret-recovery/secret-handling.md`、`docs/secret-recovery/secret-recovery-spec.md`、`docs/secret-recovery/bitwarden-secrets-manager-design.md`、`docs/secret-recovery/initial-provisioning-runbook.md`、`README.md` を確認した。
- BWS access token 入力・保存: Rust provisioning path は `BwsAccessTokenInputPort` で hidden prompt / stdin pipe から `ProtectedSecret` に読み、`rust/dotfiles-cli/src/secrets/adapters/io/process.rs` の実装は token argv option / env 注入を持たない。`scripts/provision-secret-recovery-source.sh` も token 値を stdin pipe で `dotfiles` へ渡し、`set +x` と `unset` を置いているため、token 値自体の argv/env/xtrace 出力は確認されなかった。
- BWS SDK 境界: `rust/dotfiles-cli/src/secrets/support/protection/bws.rs` は access token の owned plaintext buffer を SDK login 呼び出し境界内で作成し、`ZeroizingAccessTokenLoginRequest` の Drop で zeroize する。SDK error は固定文言へ map され、token / secret value を error に含めない。
- YubiKey storage: `rust/dotfiles-cli/src/secrets/adapters/yubikey/storage_adapter.rs` は保存時に `ProtectedSecret` を `seal_for_storage` へ渡し、読取時も `SecretSession` と `ProtectedSecret` で扱う。`verify-yubikey --check bws` は local storage 検証後に `bws-access-token` を on-demand で読み、BWS check 分岐の scope に閉じている。
- GPG backup envelope: `rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs` は `gpg-secret-key-backup` envelope の schema、`metadata.primary_fingerprint` lowercase hex 40 文字、recipient serial + public key fingerprint matching、ciphertext nonce/body/tag を domain 検証し、parse error へ secret 本文を含めない。`run_verify_yubikey_with.rs` の BWS check は unwrap を呼ばず、一致 recipient 存在までで recoverability を確認する。
- GitHub/BWS 操作: Rust の BWS create/update は固定 secret key を adapter で設定し、`password-store-remote` value を SDK request に渡すが report へ出さない。GitHub 操作を含む provisioning script は token 値を argv/env に載せない。
- README/runbook/spec/design: 文書は BWS 登録・更新用 token と YubiKey 保存用復旧 token を同一値にしないこと、token を argv/log/shell history/env/temp file に残さないこと、`gpg-secret-key-backup` を encrypted envelope として扱うこと、`password-store-remote` は credential ではないが出力に漏らさないことを明記している。

Finding 1: `scripts/provision-secret-recovery-source.sh` が private `password-store` repository の所在を通常出力へ出す。
- 該当箇所: `scripts/provision-secret-recovery-source.sh:167` は `log "GitHub に private repo を作成: $PASS_REPO"` で owner/repo を出力する。`scripts/provision-secret-recovery-source.sh:178` は `pass git push -u origin main` の stdout/stderr を抑止しておらず、通常の Git push 出力で remote URL / repo 所在が terminal log や CI log に残り得る。
- セキュリティ根拠: `docs/secret-recovery/bitwarden-secrets-manager-design.md` は `password-store-remote` を credential ではないが private repository の所在として扱い、ログ・エラー本文・診断出力へ含めない契約にしている。provisioning script はこの private repo 所在を出力するため、secret-handling 上の漏えい経路を閉じ切れていない。
- 解消条件: private repo owner/repo / clone URL を通常ログへ出さない。GitHub repo 作成・remote 設定・push は必要最小限の固定文言にし、`git push` の出力は捕捉して失敗時も URL を含まない固定 error へ変換する。
- Rationale: これは private repository 所在の出力経路であり、`スコープ外` や `運用でログを保存しない` として downgrade してはならない。

Finding 2: Rust 側コメントが、BWS 登録用 token と YubiKey 保存用 `bws-access-token` に同じ値を使う運用を許容している。
- 該当箇所: `rust/dotfiles-cli/src/secrets/ports/io.rs:43-45`、`rust/dotfiles-cli/src/secrets/domain/commands.rs:286-289`、`rust/dotfiles-cli/src/secrets/application/run_provision_password_store_remote.rs:23-25` / `:49-50`、`rust/dotfiles-cli/src/secrets/application/run_register_gpg_backup_primary.rs:22-24` / `:58-59`、`rust/dotfiles-cli/src/secrets/application/run_add_gpg_backup_spare.rs:22-24` / `:67-68`。
- セキュリティ根拠: `README.md`、`docs/secret-recovery/secret-recovery-spec.md`、`docs/secret-recovery/bitwarden-secrets-manager-design.md`、`docs/secret-recovery/initial-provisioning-runbook.md` は、BWS 登録・更新用 token と YubiKey に保存する復旧用 token を同一値にしないこと、YubiKey には復旧時に必要 secret を読める最小権限 token だけを保存することを定めている。Rust 側コメントはこの最小権限分離を逆方向に説明し、review / maintenance 時に write-capable token を YubiKey へ保存する運用を正当化し得る。
- 解消条件: Rust doc comment / inline comment を、BWS 登録・更新用 token は provisioning 入力専用で YubiKey に保存しないこと、YubiKey へ保存する復旧用 token は別値かつ最小権限であること、CLI が token 値を比較できない場合も同一値運用を許容しないこと、に統一する。
- Rationale: これは least privilege と token 分離の設計契約を弱める記述であり、実行コードで値を直接比較していないことを理由に `運用上の注意` へ downgrade してはならない。

追加確認:
- `rg` による既知 token / private key 断片検索では、差分対象に実 BWS token、GitHub token、private key material、GPG secret key block の混入は確認されなかった。hit は `BW_SESSION` の仕様文言、script の変数名、test fixture のダミー値に限られる。
- `git diff --check` は whitespace error なし。

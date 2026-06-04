# Bitwarden Secrets Manager 仕様適合レビュー（2026-06-04 r2）

判定: 合格
判定要約: 所見なし
根拠:
- 対象: current worktree（未コミット差分を含む） / branch `fix/bws-provisioning-inputs-issue-44` / active work item `docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`。
- レビュー担当: 仕様適合レビュー担当。`docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md` の `境界維持の観点`、`構造完了条件`、`BSM 実装義務`、`BSM 更新系実装義務`、`完了の判定条件` を現行コード・文書へ直接照合した。
- BSM 実装義務:
  - `verify-yubikey --check bws` は、BWS project を `BwsProjectName::DOTFILES_SECRET_RECOVERY.resolve_id` で一意解決し、`gpg-secret-key-backup` と `password-store-remote` の両 secret ID を解決してから typed fetch へ進む。取得成功だけではなく、`fetch_gpg_backup_envelope` 後に connected recipient を解決し `envelope.resolve_recipient` を適用しているため、BWS secret 取得成功のみを完了扱いにしていない（`rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs:179`、`:185`、`:190`、`:193`）。
  - `gpg-secret-key-backup` envelope schema は `GpgBackupEnvelope::from_json` が `version` / `metadata` / `recipients` / `ciphertext` を `deny_unknown_fields` つき wire 型から domain 値へ変換し、固定 version・固定 algorithm・recipient 1 件以上・ciphertext nonce/tag 長などを検証する（`rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs:98`、`:137`、`:144`、`:293`、`:348`）。
  - `metadata.primary_fingerprint` は wire 保存値では lowercase hex 40 文字・区切りなしを厳格検証し、runtime CLI 入力では `PrimaryFingerprint::parse` により大文字小文字・区切り混在を lowercase hex 40 文字へ正規化する。登録経路は CLI dispatch で正規化済み `PrimaryFingerprint` を作り、export 後 fingerprint と一致する場合だけ envelope 化する（`rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs:556`、`:568`、`:719`、`rust/dotfiles-cli/src/secrets/entrypoint/dispatch.rs:231`、`rust/dotfiles-cli/src/secrets/application/run_register_gpg_backup_primary.rs:86`）。
  - 接続中 YubiKey recipient matching は `ConnectedYubiKey` が serial と `public_key_fingerprint` を持ち、`EnvelopeRecipient::matches` が両方一致のみを成立条件にしている。`run_bws_check` は `resolve_connected_recipient(serial)` の結果を `envelope.resolve_recipient` へ渡し、unwrap なしで一致 recipient の存在を検証する（`rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs:88`、`:234`、`:477`、`rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs:193`）。
  - unwrap-free recoverability check は `verify-yubikey --check bws` の BWS 分岐で `gpg_recipient.unwrap_dek()` を呼ばず、connected recipient identity の解決と matching だけで判定している。単体テストも unwrap 呼び出し 0 回と recoverability 未成立失敗を固定している（`rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs:190`、`:193`、`:512`、`:628`）。
  - integration test は BWS check 成功、invalid schema、invalid primary fingerprint、recipient mismatch、recoverability 不成立を CLI 経路で検証している（`rust/dotfiles-cli/tests/secrets_cli.rs:469`、`:498`、`:518`、`:540`、`:560`）。
- BSM 更新系 stale overwrite prevention:
  - `gpg-backup add-spare` は最初に `fetch_gpg_backup_envelope` で `(envelope, guard)` を取得し、recipient 追加後に `update_gpg_backup_envelope_if_unchanged(..., &guard)` だけで更新している（`rust/dotfiles-cli/src/secrets/application/run_add_gpg_backup_spare.rs:80`、`:118`）。
  - real adapter は SDK `revision_date` を guard 化し、取得不可時は exact secret value bytes の SHA-256 digest を fallback にする。更新直前に secret を再取得して current guard を作り、`expected_guard.ensure_matches(&current_guard)` が通った場合だけ SDK update を実行する（`rust/dotfiles-cli/src/secrets/adapters/bw.rs:104`、`:118`、`:161`、`:173`、`:180`）。
  - domain guard は revision と value digest の両方を表し、種類違いまたは値違いを stale update として拒否する。`version` や `metadata.primary_fingerprint` だけを防止条件にしていない（`rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs:503`、`:520`、`:531`、`:541`）。
  - internal stub adapter も exact value bytes digest を更新直前に再計算して guard 比較しており、CLI integration path の観測と実 port 契約が同じ stale overwrite rule を通る（`rust/dotfiles-cli/src/secrets/adapters/bw/internal_stub.rs:89`、`:145`、`:161`）。
- project gate:
  - provisioning 系 `gpg-backup register`、`gpg-backup add-spare`、`pass-remote register` は project を作成せず、`BwsProjectName::DOTFILES_SECRET_RECOVERY.resolve_id(list_bws_projects(...))` により project `dotfiles-secret-recovery` が 0 件または複数件なら停止する（`rust/dotfiles-cli/src/secrets/application/run_register_gpg_backup_primary.rs:58`、`rust/dotfiles-cli/src/secrets/application/run_add_gpg_backup_spare.rs:67`、`rust/dotfiles-cli/src/secrets/application/run_provision_password_store_remote.rs:49`、`rust/dotfiles-cli/src/secrets/domain/bws.rs:119`）。
  - README / design / runbook は project 手動作成と「登録・更新用 token から同名 project が 1 件だけ見えること」を gate として説明し、`dotfiles` / script が project を作成しないことを明記している（`README.md:111`、`docs/secret-recovery/bitwarden-secrets-manager-design.md:41`、`:118`、`docs/secret-recovery/initial-provisioning-runbook.md:85`、`scripts/provision-secret-recovery-source.sh:195`）。
- `organization` / `machine` / `service account` / `dotfiles-secret-recovery-reader` を user-facing 前提にしないこと:
  - `rg` で現行対象を検索し、`dotfiles-secret-recovery-reader` と `service account` の user-facing 前提は検出されなかった。`machine` は machine-readable の説明に限られる。
  - `organization_id` は Bitwarden SDK request field として adapter 内で access token から解決した scope ID を使うだけで、利用者に organization 入力を要求しない。該当説明とテストも adapter-local scope として固定している（`rust/dotfiles-cli/src/secrets/adapters/bw.rs:302`、`:308`、`:316`、`:360`）。
  - `README.md`、`docs/secret-recovery/bitwarden-secrets-manager-design.md`、`docs/secret-recovery/initial-provisioning-runbook.md` は個人用 BWS access token、手動 project gate、復旧用 read token の最小権限を前提にしており、固定 service account 名や固定 organization/machine 名を利用者操作として要求していない（`README.md:111`、`docs/secret-recovery/bitwarden-secrets-manager-design.md:43`、`:62`、`docs/secret-recovery/initial-provisioning-runbook.md:87`）。
- 構造完了条件・境界維持:
  - SDK 型と API 呼び出しは `rust/dotfiles-cli/src/secrets/adapters/bw.rs` に閉じ、port は domain ID/value と `ProtectedSecret` を受け渡す。domain の `bws.rs` / `gpg_backup.rs` は SDK 型や I/O 型へ依存しない（`rust/dotfiles-cli/src/secrets/adapters/bw.rs:22`、`rust/dotfiles-cli/src/secrets/ports/bw.rs:36`、`rust/dotfiles-cli/src/secrets/domain/bws.rs:1`、`rust/dotfiles-cli/src/secrets/domain/gpg_backup.rs:1`）。
  - 固定 project / secret name、一意解決、0 件/複数件 failure、recipient matching、stale guard は domain/application/port 契約に置かれ、`support` への移動で解消扱いにしていない（`rust/dotfiles-cli/src/secrets/domain/bws.rs:15`、`:119`、`rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs:162`）。
  - app 層の BWS/verify テストは `mockall` port mock を直接使い、internal test stub feature bridge や app 層共有 test support file を参照していない（`rust/dotfiles-cli/src/secrets/application/run_verify_yubikey_with.rs:263`）。
- 完了条件:
  - 未コミット worktree を含む対象差分は `git status --short --branch` と対象ファイル読取で再特定した。
  - 本レビューでは仕様適合の直接照合を行った。テスト・build コマンドは実行していないため、実検証の通過有無そのものは本レビュー判定の代替根拠にしていない。

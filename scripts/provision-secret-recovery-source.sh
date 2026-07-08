#!/usr/bin/env bash
#
# secret-recovery のソース（プロビジョニング元）環境での初期登録を補助するスクリプト。
#
# 既存 password-store の .gpg-id recipient とローカル GPG secret key を前提に、
# password-store の GitHub remote 作成/設定/push、GitHub SSH 鍵登録、
# BWS への復旧用 secret 登録、YubiKey への復旧用 bws-access-token 保存を扱う。
# BWS project `dotfiles-secret-recovery` は事前に Bitwarden Secrets Manager 側で作成し、
# provisioning token から 1 件だけ見える状態にしてからこの script の BWS 登録段階へ進む。
# YubiKey の serial は `PROVISIONING_YUBIKEY_SERIAL` または可視プロンプトで指定する。
#
# 正本: docs/secret-recovery/initial-provisioning-runbook.md と spec/design。
# 使い方（実端末で。PIN/secret/touch/passphrase の対話入力あり）:
#   bash scripts/provision-secret-recovery-source.sh

set -euo pipefail
# BWS access token は command substitution と pipe で扱うため、`bash -x` 実行時でも値を trace へ
# 出さない。復元はしない。
{ set +x; } 2>/dev/null || true
trap 'unset PROVISIONING_BWS_TOKEN RECOVERY_BWS_TOKEN' EXIT

# ─── 設定（既定値を自動導出。必要なら環境変数で上書き）───
GPG_ALGO_PRIMARY="${GPG_ALGO_PRIMARY:-ed25519}"
GPG_ALGO_ENCRYPT="${GPG_ALGO_ENCRYPT:-cv25519}"
PASS_REPO="${PASS_REPO:-}"               # 例: <owner>/password-store。未指定なら GitHub ユーザーから導出
PROVISIONING_YUBIKEY_SERIAL="${PROVISIONING_YUBIKEY_SERIAL:-}" # primary recipient と bws-access-token 保存に使う YubiKey serial
SPARE_YUBIKEY_SERIAL="${SPARE_YUBIKEY_SERIAL:-}" # 任意。指定時は gpg-backup add-spare まで実行する
# ──────────────────────────────────────────────────────

log()   { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[!]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }
pause() {
  printf '\n\033[1;35m[手動]\033[0m %s\n' "$*"
  IFS= read -r -p '    完了したら Enter: ' _ </dev/tty
}

require() { command -v "$1" >/dev/null 2>&1 || die "必要なコマンドが見つかりません: $1"; }
for c in gpg git gh pass dotfiles; do require "$c"; done

preflight_github_ssh_key_scope() {
  gh api user/keys --paginate --jq 'length' >/dev/null 2>&1 \
    || die "GitHub SSH public key API の事前確認に失敗しました。gh の active account に admin:public_key scope が必要です: gh auth refresh -h github.com -s admin:public_key"
}

GH_LOGIN="$(gh api user --jq .login 2>/dev/null)" \
  || die "gh の active account で GitHub API 認証に失敗しました（gh auth login または gh auth switch）"
[ -n "$GH_LOGIN" ] \
  || die "gh の active account で GitHub API 認証に失敗しました（gh auth login または gh auth switch）"
preflight_github_ssh_key_scope

password_store_dir() {
  if [ -n "${PASSWORD_STORE_DIR:-}" ]; then
    printf '%s' "$PASSWORD_STORE_DIR"
  else
    [ -n "${HOME:-}" ] || die "HOME が未設定です（PASSWORD_STORE_DIR を指定するか HOME を設定してください）"
    printf '%s/.password-store' "$HOME"
  fi
}
password_store_gpg_id_file() {
  printf '%s/.gpg-id' "$(password_store_dir)"
}
recipient_secret_key_fingerprint() {
  gpg --list-secret-keys --with-colons --fingerprint "$1" 2>/dev/null \
    | awk -F: '/^fpr:/{ print $10; exit }'
}
gpg_uid_for_fingerprint() {
  gpg --list-secret-keys --with-colons "$1" 2>/dev/null \
    | awk -F: '/^uid:/{ print $10; exit }'
}
verify_password_store_recipients() {
  local gpg_id_file
  gpg_id_file="$(password_store_gpg_id_file)"
  [ -s "$gpg_id_file" ] \
    || die "password-store が未初期化です（.gpg-id が存在しないか空です）"

  local primary_fingerprint=""
  local recipient fingerprint
  while IFS= read -r recipient; do
    [ -n "$recipient" ] || continue
    case "$recipient" in \#*) continue ;; esac
    fingerprint="$(recipient_secret_key_fingerprint "$recipient")"
    [ -n "$fingerprint" ] \
      || die "password-store recipient の秘密鍵がローカルにありません。import/generate が必要です: $recipient"
    if [ -z "$primary_fingerprint" ]; then
      primary_fingerprint="$fingerprint"
      continue
    fi
    [ "$primary_fingerprint" = "$fingerprint" ] \
      || die "password-store .gpg-id に異なる recipient が複数あります。primary fingerprint を一意に決められません"
  done < "$gpg_id_file"
  [ -n "$primary_fingerprint" ] \
    || die "password-store .gpg-id から有効な recipient を解決できません"
  printf '%s' "$primary_fingerprint"
}
confirm_password_store_primary_fingerprint() {
  local resolved_fingerprint
  resolved_fingerprint="$(verify_password_store_recipients)"
  [ -n "${resolved_fingerprint:-}" ] \
    || die "password-store .gpg-id から primary fingerprint を解決できません"
  [ "$resolved_fingerprint" = "$PRIMARY_FINGERPRINT" ] \
    || die "password-store .gpg-id の recipient が変更されています。解決済み primary fingerprint と一致しません"
  log "password-store .gpg-id recipient の秘密鍵を再確認済み"
}
read_bws_access_token() {
  local label="$1"
  if [ -t 0 ]; then
    local token
    printf '%s: ' "$label" >/dev/tty
    IFS= read -r -s token </dev/tty
    printf '\n' >/dev/tty
    [ -n "$token" ] || die "BWS access token が空です"
    printf '%s' "$token"
  else
    local token
    IFS= read -r token
    [ -n "$token" ] || die "stdin から BWS access token を読めません"
    printf '%s' "$token"
  fi
}
run_dotfiles_with_bws_access_token() {
  printf '%s\n' "$PROVISIONING_BWS_TOKEN" | dotfiles "$@"
}
store_recovery_bws_access_token() {
  local serial="$1"
  printf '%s\n' "$RECOVERY_BWS_TOKEN" | dotfiles secrets yubikey put bws-access-token --stdin --serial "$serial"
}
provisioning_yubikey_serial() {
  if [ -n "${PROVISIONING_YUBIKEY_SERIAL:-}" ]; then
    printf '%s' "$PROVISIONING_YUBIKEY_SERIAL"
    return
  fi
  local serial
  printf 'YubiKey serial for bws-access-token storage: ' >/dev/tty
  IFS= read -r serial </dev/tty
  [ -n "$serial" ] || die "YubiKey serial が空です"
  case "$serial" in
    *[!0-9]*) die "YubiKey serial は数字で指定してください" ;;
  esac
  printf '%s' "$serial"
}
optional_spare_yubikey_serial() {
  if [ -n "${SPARE_YUBIKEY_SERIAL:-}" ]; then
    printf '%s' "$SPARE_YUBIKEY_SERIAL"
    return
  fi
  [ -t 0 ] || return 0
  local serial
  printf 'Spare YubiKey serial for gpg-backup add-spare (blank to skip): ' >/dev/tty
  IFS= read -r serial </dev/tty
  [ -n "$serial" ] || return 0
  case "$serial" in
    *[!0-9]*) die "spare YubiKey serial は数字で指定してください" ;;
  esac
  printf '%s' "$serial"
}

[ -z "${PASS_REPO:-}" ] && PASS_REPO="${GH_LOGIN}/password-store"
PASS_CLONE_URL="git@github.com:${PASS_REPO}.git"
PASSWORD_STORE_ROOT="$(password_store_dir)"
ensure_password_store_remote() {
  if pass git remote get-url origin >/dev/null 2>&1; then
    local current_origin
    current_origin="$(pass git remote get-url origin)"
    [ "$current_origin" = "$PASS_CLONE_URL" ] \
      || die "password-store の origin remote が想定と異なります"
  else
    pass git remote add origin "$PASS_CLONE_URL"
  fi
}

# ── 1. password-store recipient の GPG secret key 確認 ──
PRIMARY_FINGERPRINT="$(verify_password_store_recipients)"
[ -n "${PRIMARY_FINGERPRINT:-}" ] \
  || die "password-store .gpg-id から primary fingerprint を解決できません"
GPG_OWNER="$(gpg_uid_for_fingerprint "$PRIMARY_FINGERPRINT")"
if [ -n "${GPG_OWNER:-}" ]; then
  log "password-store .gpg-id recipient の秘密鍵を使用: ${PRIMARY_FINGERPRINT} （${GPG_OWNER}）"
else
  log "password-store .gpg-id recipient の秘密鍵を使用: ${PRIMARY_FINGERPRINT}"
fi
have_cap() { gpg --list-keys --with-colons "${PRIMARY_FINGERPRINT}" | awk -F: -v c="$1" '/^sub:/{ if (index($12,c)) f=1 } END{ exit f?0:1 }'; }
have_cap e || { log "encryption subkey を追加"; gpg --quick-add-key "${PRIMARY_FINGERPRINT}" "$GPG_ALGO_ENCRYPT" encrypt never; }
have_cap a || { log "authentication subkey を追加"; gpg --quick-add-key "${PRIMARY_FINGERPRINT}" "$GPG_ALGO_PRIMARY" auth never; }
have_cap s || { log "signing subkey を追加"; gpg --quick-add-key "${PRIMARY_FINGERPRINT}" "$GPG_ALGO_PRIMARY" sign never; }
log "GPG 鍵構成を確認済み"
warn "この鍵のバックアップと revocation certificate を別経路で保管してください。"
YUBIKEY_SERIAL="$(provisioning_yubikey_serial)"
SPARE_SERIAL="$(optional_spare_yubikey_serial)"

# ── 2. GitHub への SSH 公開鍵登録（authentication subkey 由来）──
log "authentication subkey 由来 SSH 公開鍵を GitHub に登録"
SSH_PUB="$(dotfiles gpg export-ssh-public-key --primary-fingerprint "${PRIMARY_FINGERPRINT}")"
gh ssh-key list 2>/dev/null | grep -qF "$(printf '%s' "$SSH_PUB" | awk '{print $2}')}" \
  || printf '%s\n' "$SSH_PUB" | gh ssh-key add - --title "dotfiles-gpg-auth-$(date +%Y%m%d)"

# ── 3. private password-store repository の remote 設定・push ──
gh repo view "$PASS_REPO" >/dev/null 2>&1 || {
  log "GitHub に private password-store repository を作成"
  gh repo create "$PASS_REPO" --private --disable-issues --disable-wiki >/dev/null 2>&1 \
    || die "GitHub private password-store repository の作成に失敗しました"
}
REPO_IS_PRIVATE="$(gh repo view "$PASS_REPO" --json isPrivate --jq .isPrivate 2>/dev/null)" \
  || die "GitHub password-store repository の visibility 確認に失敗しました"
[ "$REPO_IS_PRIVATE" = "true" ] \
  || die "GitHub password-store repository が private ではありません。private repository を指定してください"
if [ ! -d "$PASSWORD_STORE_ROOT/.git" ]; then
  log "既存 password-store を Git repository として初期化して remote へ push"
  pass git init >/dev/null 2>&1 || true
  pass git branch -M main >/dev/null 2>&1 || true
  PASS_PUSH_BRANCH="main"
  PASS_PUSH_MODE="set-upstream"
else
  log "既存 password-store Git repository を使用"
  PASS_PUSH_BRANCH="$(pass git branch --show-current 2>/dev/null || true)"
  [ -n "$PASS_PUSH_BRANCH" ] \
    || die "既存 password-store repository の現在 branch を解決できません。detached HEAD の場合は push 前に branch へ checkout してください"
  if pass git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' >/dev/null 2>&1; then
    PASS_PUSH_MODE="current-upstream"
  else
    PASS_PUSH_MODE="set-upstream"
  fi
fi
ensure_password_store_remote
pass git add -A >/dev/null 2>&1 || true
if pass git diff --cached --quiet >/dev/null 2>&1; then
  log "password-store に新規 commit 対象はありません"
else
  pass git commit -m "Initialize password-store" >/dev/null 2>&1 \
    || die "password-store Git commit に失敗しました。user.name/user.email/signing/hook 設定を確認してください"
fi
if [ "$PASS_PUSH_MODE" = "current-upstream" ]; then
  pass git push >/dev/null 2>&1 \
    || die "password-store Git repository の push に失敗しました。remote 設定と GitHub SSH 認証を確認してください"
else
  pass git push -u origin "$PASS_PUSH_BRANCH" >/dev/null 2>&1 \
    || die "password-store Git repository の push に失敗しました。remote 設定と GitHub SSH 認証を確認してください"
fi
confirm_password_store_primary_fingerprint

# ── 4. BWS への復旧用 secret 登録 ──
pause "Bitwarden Secrets Manager 側で project 'dotfiles-secret-recovery' を作成済みで、これから入力する BWS 登録・更新用 token から同名 project が 1 件だけ見えることを確認してください。project 作成はこの script / dotfiles CLI では行いません。"
PROVISIONING_BWS_TOKEN="$(read_bws_access_token 'BWS provisioning access token for create/update')"

log "BWS に password-store-remote を登録"
pause "BWS の password-store-remote をこれから設定します。既存 secret がある場合も、現在の PASS_REPO / GitHub active account から導出した復旧先が意図した private password-store repository であることを確認してください。repository 所在はログへ表示しません。"
run_dotfiles_with_bws_access_token secrets pass-remote register --url "$PASS_CLONE_URL" --yes

log "BWS に gpg-secret-key-backup を登録"
run_dotfiles_with_bws_access_token secrets gpg-backup register --primary-fingerprint "$PRIMARY_FINGERPRINT" --serial "$YUBIKEY_SERIAL"
if [ -n "${SPARE_SERIAL:-}" ]; then
  log "BWS の gpg-secret-key-backup に spare recipient を追加"
  run_dotfiles_with_bws_access_token secrets gpg-backup add-spare --unwrap-serial "$YUBIKEY_SERIAL" --spare-serial "$SPARE_SERIAL" --yes
else
  warn "spare YubiKey serial が未指定のため gpg-backup add-spare は未実行です。spare で復旧可能にするには後で dotfiles secrets gpg-backup add-spare を実行してください。"
fi

# ── 5. YubiKey への復旧用 BWS read token 保存 ──
RECOVERY_BWS_TOKEN="$(read_bws_access_token 'BWS recovery/read access token for YubiKey storage')"
[ "$RECOVERY_BWS_TOKEN" != "$PROVISIONING_BWS_TOKEN" ] \
  || die "復旧用 BWS access token が登録・更新用 token と同一です。YubiKey には最小権限の復旧用 token だけを保存してください"
log "復旧用 bws-access-token を YubiKey に保存"
store_recovery_bws_access_token "$YUBIKEY_SERIAL"
if [ -n "${SPARE_SERIAL:-}" ]; then
  log "復旧用 bws-access-token を spare YubiKey にも保存"
  store_recovery_bws_access_token "$SPARE_SERIAL"
fi
unset PROVISIONING_BWS_TOKEN RECOVERY_BWS_TOKEN

# ── 手動: 各サービスの YubiKey 物理登録 ──
pause "次を各サービスの UI / 管理画面で行ってください（API でリモート登録できない物理/アカウント操作）:
    - Bitwarden Password Manager: アカウント 2FA に primary/spare の YubiKey を登録。recovery code 保管。
    - GitHub: security key として primary/spare を登録。
    - Google / Apple 等: primary/spare を FIDO2/passkey 登録。recovery code 保管。"

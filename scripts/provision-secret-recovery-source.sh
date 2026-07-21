#!/usr/bin/env bash
#
# secret-recovery のソース（プロビジョニング元）環境での初期登録を補助するスクリプト。
#
# 既存 password-store の .gpg-id recipient とローカル GPG secret key を前提に、
# password-store の GitHub remote 作成/設定/push、GitHub SSH 鍵登録、
# YubiKey への BWS access token 保存、BWS への復旧用 secret 登録を扱う。
# BWS project `dotfiles-secret-recovery` は事前に Bitwarden Secrets Manager 側で作成し、
# YubiKey に保存する token から 1 件だけ見える状態にしてからこの script の BWS 登録段階へ進む。
# YubiKey の serial は接続中のデバイスから自動検出する。明示指定する場合は
# `PROVISIONING_YUBIKEY_SERIAL` / `SPARE_YUBIKEY_SERIAL` 環境変数で指定する。
#
# 正本: docs/secret-recovery/initial-provisioning-runbook.md と spec/design。
# 使い方（実端末で。secret/touch/passphrase の対話入力あり。`setup` / `put` / `clear` を
# 呼ぶ段階では、子 `dotfiles` command が controlling TTY から PIV 管理 PIN を hidden prompt
# で読む。PIN はこの script の stdin / argv / environment では扱わない）:
#   bash scripts/provision-secret-recovery-source.sh
#   bash scripts/provision-secret-recovery-source.sh --repo-head

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
USE_REPO_HEAD=0
case "${DOTFILES_PROVISION_USE_REPO_HEAD:-}" in
  1) USE_REPO_HEAD=1 ;;
  "") ;;
  *) die "DOTFILES_PROVISION_USE_REPO_HEAD は 1 だけを受け付けます" ;;
esac
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo-head) USE_REPO_HEAD=1; shift ;;
    -h|--help) printf 'Usage: %s [--repo-head]\n' "$0"; exit 0 ;;
    *) die "未知の引数です: $1" ;;
  esac
done
# BWS access token は command substitution と pipe で扱うため、`bash -x` 実行時でも値を trace へ
# 出さない。復元はしない。
{ set +x; } 2>/dev/null || true
trap 'unset BWS_ACCESS_TOKEN' EXIT

# ─── 設定（既定値を自動導出。必要なら環境変数で上書き）───
GPG_ALGO_PRIMARY="${GPG_ALGO_PRIMARY:-ed25519}"
GPG_ALGO_ENCRYPT="${GPG_ALGO_ENCRYPT:-cv25519}"
PASS_REPO="${PASS_REPO:-}"               # 例: <owner>/password-store。未指定なら GitHub ユーザーから導出
PROVISIONING_YUBIKEY_SERIAL="${PROVISIONING_YUBIKEY_SERIAL:-}" # primary recipient と bitwarden-client-secret 保存に使う YubiKey serial
SPARE_YUBIKEY_SERIAL="${SPARE_YUBIKEY_SERIAL:-}" # 任意。指定時は gpg-backup add-spare まで実行する
# ──────────────────────────────────────────────────────

# `--repo-head` 時も、`put --stdin` の標準入力だけは呼び出し元の pipe をそのまま渡す。
# 他の CLI 呼び出しは後段の `dotfiles` wrapper が controlling terminal を使う。
# 正本: docs/secret-recovery/initial-provisioning-runbook.md。Cargo の binary target 選択は
# https://doc.rust-lang.org/cargo/reference/cargo-targets.html#binaries を根拠に `--bin` で明示する。
# feature 専用の internal test stub ではなく、利用者向け production `dotfiles` を起動して
# プロビジョニングの実経路を選ぶためである。
run_dotfiles_from_repo_head() {
  (cd "$REPO_ROOT" && direnv exec . cargo run -p dotfiles-cli --bin dotfiles -- "$@")
}
dotfiles_with_stdin() {
  if [ "$USE_REPO_HEAD" -eq 1 ]; then
    run_dotfiles_from_repo_head "$@"
  else
    dotfiles "$@"
  fi
}

log()   { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[!]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }
pause() {
  printf '\n\033[1;35m[手動]\033[0m %s\n' "$*"
  IFS= read -r -p '    完了したら Enter: ' _ </dev/tty
}

preflight_github_ssh_key_scope() {
  gh api user/keys --paginate --jq 'length' >/dev/null 2>&1 \
    || die "GitHub SSH public key API の事前確認に失敗しました。gh の active account に admin:public_key scope が必要です: gh auth refresh -h github.com -s admin:public_key"
}

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
# TTY secret prompt の共通 I/O 契約。PIV PIN は子 Rust command が同じ契約で /dev/tty から受け取り、
# この script は BWS token だけを扱う。値は mask 以外へ表示せず、stdin token とは混在させない。
write_secret_tty() { printf '%s' "$1" >/dev/tty; }
secret_tty_state() { stty -g </dev/tty; }
prepare_secret_tty() { stty -echo -icanon -isig min 1 time 0 </dev/tty; }
restore_secret_tty() { stty "$1" </dev/tty; }
read_secret_tty_byte() { IFS= read -r -s -n 1 REPLY </dev/tty; }

read_masked_tty_secret() {
  local label="$1" token="" byte="" tty_state=""
  tty_state="$(secret_tty_state)" || die "controlling TTY の状態を取得できません"
  write_secret_tty "${label}: "
  prepare_secret_tty || die "controlling TTY を secret input 用に設定できません"
  # This function is called through command substitution, so the signal handler restores the
  # terminal in that subshell before terminating it. No token text is sent to the display path.
  trap 'restore_secret_tty "$tty_state"; exit 130' HUP INT TERM
  while :; do
    if ! read_secret_tty_byte; then
      restore_secret_tty "$tty_state"
      trap - HUP INT TERM
      die "controlling TTY から BWS access token を読めません"
    fi
    byte="$REPLY"
    case "$byte" in
      '')
        write_secret_tty $'\n'
        break
        ;;
      $'\b'|$'\177')
        if [ -n "$token" ]; then
          token="${token%?}"
          write_secret_tty $'\b \b'
        fi
        ;;
      $'\003')
        restore_secret_tty "$tty_state"
        trap - HUP INT TERM
        die "BWS access token の入力を中断しました"
        ;;
      *)
        token+="$byte"
        write_secret_tty '*'
        ;;
    esac
  done
  restore_secret_tty "$tty_state"
  trap - HUP INT TERM
  [ -n "$token" ] || die "BWS access token が空です"
  printf '%s' "$token"
}

read_bws_access_token() {
  if [ -t 0 ]; then
    read_masked_tty_secret 'BWS access token for YubiKey bitwarden-client-secret storage'
  else
    local token
    IFS= read -r token
    [ -n "$token" ] || die "stdin から BWS access token を読めません"
    printf '%s' "$token"
  fi
}
store_bws_access_token() {
  local serial="$1"
  if [ -n "$serial" ]; then
    printf '%s\n' "$BWS_ACCESS_TOKEN" | dotfiles_with_stdin secrets yubikey put bitwarden-client-secret --stdin --serial "$serial"
  else
    printf '%s\n' "$BWS_ACCESS_TOKEN" | dotfiles_with_stdin secrets yubikey put bitwarden-client-secret --stdin
  fi
}
inspect_bws_access_token_storage() {
  local serial="$1"
  if [ -n "$serial" ]; then
    dotfiles secrets yubikey status --serial "$serial"
  else
    dotfiles secrets yubikey status
  fi
}
initialize_bws_access_token_storage() {
  local serial="$1"
  if [ -n "$serial" ]; then
    dotfiles secrets yubikey setup --serial "$serial"
  else
    dotfiles secrets yubikey setup
  fi
}
clear_bws_access_token_storage() {
  local serial="$1"
  if [ -n "$serial" ]; then
    dotfiles secrets yubikey clear --serial "$serial" --yes
  else
    dotfiles secrets yubikey clear --yes
  fi
}
ensure_bws_access_token_stored() {
  local role="$1"
  local serial="$2"
  local stored_names
  # `dotfiles secrets yubikey status` の予約 storage 不整合専用終了コード。
  # stderr は分類に使わない。USB/PCSC/serial 等の任意失敗は fail-closed で停止する。
  local readonly invalid_storage_status=42
  # `put` が完全な未初期化を観測した専用終了コード。正常な manifest の任意 subset は
  # `put` をそのまま許可し、setup を再実行しない。
  local readonly uninitialized_storage_status=43
  if stored_names="$(inspect_bws_access_token_storage "$serial")"; then
    if printf '%s\n' "$stored_names" | grep -Fxq 'bitwarden-client-secret'; then
      log "${role} YubiKey の bitwarden-client-secret は保存済みです"
      return 0
    fi
  else
    local status_result=$?
    if [ "$status_result" -ne "$invalid_storage_status" ]; then
      die "${role} YubiKey の bitwarden-client-secret の保存状況を確認できません"
    fi
    log "${role} YubiKey の不正な secret storage を clear"
    clear_bws_access_token_storage "$serial" \
      || die "${role} YubiKey の secret storage を clear できません"
    # clear は slot 82 再生成と空の v2 manifest 確定までを一つの管理操作として完了する。
    # ここで setup を続けると正常な no-op storage に余分な PIN session を要求するだけなので再実行しない。
  fi
  if [ -z "${BWS_ACCESS_TOKEN:-}" ]; then
    BWS_ACCESS_TOKEN="$(read_bws_access_token 'BWS access token for YubiKey bitwarden-client-secret storage')"
  fi
  log "BWS access token を ${role} YubiKey の bitwarden-client-secret に保存"
  if store_bws_access_token "$serial"; then
    return 0
  else
    # `if` 複文の終了値ではなく、put 自身の失敗だけを判定する。43 以外は
    # 初期化へ進めず fail-closed にする。
    local put_result=$?
  fi
  if [ "$put_result" -ne "$uninitialized_storage_status" ]; then
    die "${role} YubiKey の bitwarden-client-secret を保存できません"
  fi
  log "${role} YubiKey の完全に空の secret storage を初期化"
  initialize_bws_access_token_storage "$serial" \
    || die "${role} YubiKey の空の secret storage を初期化できません"
  store_bws_access_token "$serial" \
    || die "${role} YubiKey の初期化後に bitwarden-client-secret を保存できません"
}

# shell test は production の初期化・外部コマンド呼び出しなしで保存判定の分岐だけを検証する。
if [ "${DOTFILES_PROVISION_SOURCE_ONLY:-}" = 1 ] && [ "${BASH_SOURCE[0]}" != "$0" ]; then
  return 0
fi

require() { command -v "$1" >/dev/null 2>&1 || die "必要なコマンドが見つかりません: $1"; }
for c in gpg git gh pass; do require "$c"; done
if [ "$USE_REPO_HEAD" -eq 1 ]; then
  require cargo
  require direnv
  dotfiles() {
    if { exec 9</dev/tty; } 2>/dev/null; then
      run_dotfiles_from_repo_head "$@" <&9
      local s=$?; exec 9<&-; return "$s"
    else
      run_dotfiles_from_repo_head "$@" </dev/null
    fi
  }
else
  require dotfiles
fi

GH_LOGIN="$(gh api user --jq .login 2>/dev/null)" \
  || die "gh の active account で GitHub API 認証に失敗しました（gh auth login または gh auth switch）"
[ -n "$GH_LOGIN" ] \
  || die "gh の active account で GitHub API 認証に失敗しました（gh auth login または gh auth switch）"
preflight_github_ssh_key_scope

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
YUBIKEY_SERIAL="${PROVISIONING_YUBIKEY_SERIAL:-}"
SPARE_SERIAL="${SPARE_YUBIKEY_SERIAL:-}"

# ── 2. GitHub への SSH 公開鍵登録（authentication subkey 由来）──
log "authentication subkey 由来 SSH 公開鍵を GitHub に登録"
SSH_PUB="$(dotfiles gpg export-ssh-public-key --primary-fingerprint "${PRIMARY_FINGERPRINT}")"
gh ssh-key list 2>/dev/null | grep -qF "$(printf '%s' "$SSH_PUB" | awk '{print $2}')" \
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

# ── 4. YubiKey への BWS access token 保存 ──
pause "Bitwarden Secrets Manager 側で project 'dotfiles-secret-recovery' を作成済みで、対象 YubiKey に保存済みまたはこれから保存する BWS access token から同名 project が 1 件だけ見えることを確認してください。project 作成はこの script / dotfiles CLI では行いません。"
ensure_bws_access_token_stored "primary" "$YUBIKEY_SERIAL"
if [ -n "${SPARE_SERIAL:-}" ]; then
  ensure_bws_access_token_stored "spare" "$SPARE_SERIAL"
fi
unset BWS_ACCESS_TOKEN

# ── 5. BWS への復旧用 secret 登録 ──
log "BWS に password-store-remote を登録"
pause "BWS の password-store-remote をこれから設定します。既存 secret がある場合も、現在の PASS_REPO / GitHub active account から導出した復旧先が意図した private password-store repository であることを確認してください。repository 所在はログへ表示しません。"
if [ -n "${YUBIKEY_SERIAL:-}" ]; then
  dotfiles secrets pass-remote register --url "$PASS_CLONE_URL" --yes --serial "$YUBIKEY_SERIAL"
else
  dotfiles secrets pass-remote register --url "$PASS_CLONE_URL" --yes
fi

log "BWS に gpg-secret-key-backup を登録"
if [ -n "${YUBIKEY_SERIAL:-}" ]; then
  dotfiles secrets gpg-backup register --primary-fingerprint "$PRIMARY_FINGERPRINT" --serial "$YUBIKEY_SERIAL"
else
  dotfiles secrets gpg-backup register --primary-fingerprint "$PRIMARY_FINGERPRINT"
fi
if [ -n "${SPARE_SERIAL:-}" ]; then
  log "BWS の gpg-secret-key-backup に spare recipient を追加"
  if [ -n "${YUBIKEY_SERIAL:-}" ]; then
    dotfiles secrets gpg-backup add-spare --unwrap-serial "$YUBIKEY_SERIAL" --spare-serial "$SPARE_SERIAL" --yes
  else
    dotfiles secrets gpg-backup add-spare --spare-serial "$SPARE_SERIAL" --yes
  fi
else
  warn "spare YubiKey serial が未指定のため gpg-backup add-spare は未実行です。spare で復旧可能にするには後で dotfiles secrets gpg-backup add-spare を実行してください。"
fi

# ── 手動: 各サービスの YubiKey 物理登録 ──
pause "次を各サービスの UI / 管理画面で行ってください（API でリモート登録できない物理/アカウント操作）:
    - Bitwarden Password Manager: アカウント 2FA に primary/spare の YubiKey を登録。recovery code 保管。
    - GitHub: security key として primary/spare を登録。
    - Google / Apple 等: primary/spare を FIDO2/passkey 登録。recovery code 保管。"

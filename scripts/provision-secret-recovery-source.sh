#!/usr/bin/env bash
#
# secret-recovery のソース（プロビジョニング元）環境での初期登録を補助するスクリプト。
#
# 既存設定があれば検証して使い、未設定の password-store はローカル GPG secret key を
# 解決または作成して pass init する。複数候補で一意に決められない場合は停止する。
# password-store の GitHub remote 作成/設定/push、GitHub SSH 鍵登録、secret-recovery の
# personal vault/YubiKey 登録 command 起動までを扱う。この script は primary YubiKey と
# `password-store-remote` の登録までを実行し、spare 登録後に operator が
# `gpg-secret-key-backup` envelope を個人 Bitwarden vault へ投入して
# `dotfiles secrets gpg-backup register` で監査する。
#
# 事前投入 gate:
# - この script と `dotfiles secrets yubikey enroll-spare` の完了後に、個人 Bitwarden vault
#   item `gpg-secret-key-backup` へ 2 recipient 以上を含む encrypted envelope を投入する。
# - この script は envelope 本文、fingerprint、URL、secret を shell argv/stdin/env で受け取らず、
#   BWS / bw CLI login / unlock / session / project / organization 経路も使わない。
# - gate は、この script の外で実行する `dotfiles secrets gpg-backup register` が既存 item を照合して監査する。
#
# 入力規約:
# - この shell script は provisioning 入力値（vault credential、password-store remote URL、
#   GPG primary fingerprint、YubiKey 保存 secret など）を read / pipe / argv / 環境変数で受け取らない。
# - provisioning 入力値を `dotfiles` CLI へ値付き argv / pipe / stdin で中継しない。
# - `dotfiles` CLI が必要とする入力や一意解決は、CLI 側の prompt/input port または adapter に委譲する。
# - shell script は既存設定の確認、GitHub / GPG / password-store 外部 command の順序実行、
#   およびそれらの command が必要とするローカル環境値の解決だけを担う。
#
# 正本: docs/secret-recovery/initial-provisioning-runbook.md と spec/design。
# 使い方（実端末で。PIN/secret/touch/passphrase の対話入力あり）:
#   bash scripts/provision-secret-recovery-source.sh
#   bash scripts/provision-secret-recovery-source.sh --repo-head
#   DOTFILES_PROVISION_USE_REPO_HEAD=1 bash scripts/provision-secret-recovery-source.sh

set -euo pipefail
# 念のため trace は無効化する。secret / credential 入力は dotfiles CLI 側へ委譲し、この script では読まない。
{ set +x; } 2>/dev/null || true

# ─── 固定既定値。利用者入力による上書きはこの shell script では扱わない。───
GPG_ALGO_PRIMARY="ed25519"
GPG_ALGO_ENCRYPT="cv25519"
PASS_REPO=""
# ──────────────────────────────────────────────────────

log()   { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[!]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }
require() { command -v "$1" >/dev/null 2>&1 || die "必要なコマンドが見つかりません: $1"; }
mask_fingerprint() {
  printf '[redacted fingerprint]'
}
run_gpg_quick() {
  local stderr_file
  stderr_file="$(mktemp "${TMPDIR:-/tmp}/dotfiles-gpg.XXXXXX")" \
    || die "GPG 診断出力の一時退避先を作成できません"
  if gpg "$@" 2>"$stderr_file"; then
    rm -f "$stderr_file"
    return 0
  fi
  rm -f "$stderr_file"
  return 1
}

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
USE_REPO_HEAD=0
case "${DOTFILES_PROVISION_USE_REPO_HEAD:-}" in
  1) USE_REPO_HEAD=1 ;;
  "") ;;
  *) die "DOTFILES_PROVISION_USE_REPO_HEAD は 1 だけを受け付けます。それ以外は --repo-head を使わず停止します" ;;
esac
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo-head)
      USE_REPO_HEAD=1
      shift
      ;;
    -h|--help)
      printf 'Usage: %s [--repo-head]\n' "$0"
      exit 0
      ;;
    *)
      die "未知の引数です。この script は secret/credential/URL/fingerprint を argv で受け取りません"
      ;;
  esac
done

for c in gpg git gh pass; do require "$c"; done
if [ "$USE_REPO_HEAD" -eq 1 ]; then
  require cargo
  require direnv
else
  require dotfiles
fi


run_dotfiles() {
  local status
  if [ "$USE_REPO_HEAD" -eq 1 ]; then
    if { exec 9</dev/tty; } 2>/dev/null; then
      (cd "$REPO_ROOT" && direnv exec . env CARGO_TARGET_DIR=target/provision-secret-recovery-source cargo run -p dotfiles-cli -- "$@") <&9
      status=$?
      exec 9<&-
      return "$status"
    else
      (cd "$REPO_ROOT" && direnv exec . env CARGO_TARGET_DIR=target/provision-secret-recovery-source cargo run -p dotfiles-cli -- "$@") </dev/null
    fi
  elif { exec 9</dev/tty; } 2>/dev/null; then
    dotfiles "$@" <&9
    status=$?
    exec 9<&-
    return "$status"
  else
    dotfiles "$@" </dev/null
  fi
}

preflight_github_ssh_key_scope() {
  gh api user/keys --paginate --jq 'length' >/dev/null 2>&1 \
    || die "GitHub SSH public key API の事前確認に失敗しました。gh の active account には classic token なら read:public_key scope、fine-grained token なら Git SSH keys: read permission が必要です。classic token の更新例: gh auth refresh -h github.com -s read:public_key"
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
emit_gpg_secret_key_record() {
  local fingerprint="$1"
  local uid="$2"
  local subkey_capabilities="$3"
  [ -n "$fingerprint" ] || return 0
  printf '%s\t%s\t%s\n' "$fingerprint" "$uid" "$subkey_capabilities"
}
gpg_secret_key_records() {
  local eligible=0
  local fingerprint=""
  local uid=""
  local subkey_capabilities=""
  local line fields kind validity capabilities
  while IFS= read -r line; do
    IFS=: read -r -a fields <<<"$line"
    kind="${fields[0]:-}"
    validity="${fields[1]:-}"
    capabilities="${fields[11]:-}"
    case "$kind" in
      sec)
        emit_gpg_secret_key_record "$fingerprint" "$uid" "$subkey_capabilities"
        fingerprint=""
        uid=""
        subkey_capabilities=""
        eligible=0
        if [[ "$validity" != *r* && "$validity" != *e* && "$validity" != *d* && "$capabilities" != *D* ]]; then
          eligible=1
        fi
        ;;
      fpr)
        if [ "$eligible" -eq 1 ] && [ -z "$fingerprint" ]; then
          fingerprint="${fields[9]:-}"
        fi
        ;;
      uid)
        if [ "$eligible" -eq 1 ] && [ -z "$uid" ]; then
          uid="${fields[9]:-}"
        fi
        ;;
      ssb)
        if [ "$eligible" -eq 1 ] \
          && [[ "$validity" != *r* && "$validity" != *e* && "$validity" != *d* && "$capabilities" != *D* ]]; then
          subkey_capabilities="${subkey_capabilities}${capabilities}"
        fi
        ;;
    esac
  done < <(gpg --list-secret-keys --with-colons --fingerprint 2>/dev/null)
  emit_gpg_secret_key_record "$fingerprint" "$uid" "$subkey_capabilities"
}
gpg_record_for_recipient() {
  local recipient="$1"
  local matched_fingerprint=""
  local matched_uid=""
  local matched_capabilities=""
  local fingerprint uid capabilities
  while IFS=$'\t' read -r fingerprint uid capabilities; do
    [ -n "$fingerprint" ] || continue
    if [ "$recipient" = "$fingerprint" ] \
      || [ "$recipient" = "${fingerprint: -16}" ] \
      || [ "$recipient" = "$uid" ] \
      || [[ -n "$uid" && "$uid" == *"$recipient"* ]]; then
      if [ -n "$matched_fingerprint" ] && [ "$matched_fingerprint" != "$fingerprint" ]; then
        return 2
      fi
      matched_fingerprint="$fingerprint"
      matched_uid="$uid"
      matched_capabilities="$capabilities"
    fi
  done < <(gpg_secret_key_records)
  [ -n "$matched_fingerprint" ] || return 1
  printf '%s\t%s\t%s\n' "$matched_fingerprint" "$matched_uid" "$matched_capabilities"
}
recipient_secret_key_fingerprint() {
  local record
  record="$(gpg_record_for_recipient "$1")" || return 1
  printf '%s' "${record%%$'\t'*}"
}
gpg_uid_for_fingerprint() {
  local record rest uid
  record="$(gpg_record_for_recipient "$1")" || return 1
  rest="${record#*$'\t'}"
  uid="${rest%%$'\t'*}"
  printf '%s' "$uid"
}
gpg_unique_uid_for_fingerprint() {
  local fingerprint="$1" target_uid other_fingerprint other_uid capabilities
  target_uid="$(gpg_uid_for_fingerprint "$fingerprint")" || return 1
  [ -n "$target_uid" ] \
    || die "GPG key の UID を解決できません"
  while IFS=$'\t' read -r other_fingerprint other_uid capabilities; do
    [ -n "$other_fingerprint" ] || continue
    if [ "$other_uid" = "$target_uid" ] && [ "$other_fingerprint" != "$fingerprint" ]; then
      die "同一 UID を持つ GPG secret key が複数存在するため、UID 経由の GPG 操作を安全に実行できません"
    fi
  done < <(gpg_secret_key_records)
  printf '%s' "$target_uid"
}
gpg_secret_key_fingerprints() {
  local fingerprint uid capabilities
  while IFS=$'\t' read -r fingerprint uid capabilities; do
    [ -n "$fingerprint" ] || continue
    printf '%s\n' "$fingerprint"
  done < <(gpg_secret_key_records)
}
assert_eligible_gpg_primary() {
  local fingerprint="$1"
  gpg_record_for_recipient "$fingerprint" >/dev/null \
    || die "GPG secret key が revoked / expired / disabled、またはローカルで使用不能です: $(mask_fingerprint "$fingerprint")"
}
have_eligible_subkey_capability() {
  local fingerprint="$1"
  local capability="$2"
  local record capabilities
  record="$(gpg_record_for_recipient "$fingerprint")" || return 1
  capabilities="${record##*$'\t'}"
  [[ "$capabilities" == *"$capability"* ]]
}
create_gpg_secret_key() {
  local name email uid fingerprint
  name="$(git config --global user.name 2>/dev/null || true)"
  email="$(git config --global user.email 2>/dev/null || true)"
  [ -n "$name" ] || name="${USER:-dotfiles} secret recovery"
  if [ -z "$email" ]; then
    local host
    host="$(hostname 2>/dev/null || printf 'localhost')"
    email="${USER:-dotfiles}@${host}"
  fi

  uid="${name} <${email}>"
  log "password-store 用 GPG secret key を作成" >&2
  run_gpg_quick --quick-generate-key "$uid" "$GPG_ALGO_PRIMARY" cert never \
    || die "GPG secret key の作成に失敗しました"
  fingerprint="$(recipient_secret_key_fingerprint "$uid")"
  [ -n "$fingerprint" ] \
    || die "作成した GPG secret key の fingerprint を解決できません"
  assert_eligible_gpg_primary "$fingerprint"
  printf '%s' "$fingerprint"
}
select_gpg_secret_key() {
  local fingerprints=()
  while IFS= read -r fingerprint; do
    [ -n "$fingerprint" ] || continue
    fingerprints+=("$fingerprint")
  done < <(gpg_secret_key_fingerprints)
  case "${#fingerprints[@]}" in
    0)
      create_gpg_secret_key
      return
      ;;
    1)
      printf '%s' "${fingerprints[0]}"
      return
      ;;
  esac

  die "複数の GPG secret key が存在します。未初期化 password-store では対象を一意に決められません。先に password-store を対象 key で初期化してください"
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
      || die "password-store recipient の使用可能な秘密鍵がローカルにありません。import/generate または revoked / expired / disabled 状態の解消が必要です"
    assert_eligible_gpg_primary "$fingerprint"
    if [ -z "$primary_fingerprint" ]; then
      primary_fingerprint="$fingerprint"
    elif [ "$primary_fingerprint" != "$fingerprint" ]; then
      die "password-store .gpg-id に複数の GPG primary key が含まれています。対象を一意に決められません"
    fi
  done < "$gpg_id_file"
  [ -n "$primary_fingerprint" ] \
    || die "password-store .gpg-id から有効な recipient を解決できません"
  printf '%s' "$primary_fingerprint"
}
ensure_password_store_initialized() {
  local gpg_id_file fingerprint key_ref
  gpg_id_file="$(password_store_gpg_id_file)"
  if [ -s "$gpg_id_file" ]; then
    verify_password_store_recipients
    return
  fi

  fingerprint="$(select_gpg_secret_key)"
  [ -n "$fingerprint" ] \
    || die "password-store 用 GPG secret key を解決できません"
  key_ref="$(gpg_unique_uid_for_fingerprint "$fingerprint")"
  [ -n "$key_ref" ] \
    || die "password-store 用 GPG key の UID を解決できません"
  log "password-store が未初期化のため pass init を実行" >&2
  mkdir -p "$PASSWORD_STORE_ROOT"
  pass init "$key_ref" >/dev/null 2>&1 \
    || die "pass init に失敗しました"
  verify_password_store_recipients
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
PASSWORD_STORE_ROOT="$(password_store_dir)"
github_repo_from_clone_url() {
  local url="$1"
  local owner='[A-Za-z0-9]([A-Za-z0-9-]{0,37}[A-Za-z0-9])?'
  local repo='[A-Za-z0-9._-]+'
  if [[ "$url" =~ ^https://github\.com/(${owner})/(${repo})(\.git)?$ ]]; then
    case "${BASH_REMATCH[2]}" in
      .|..) return 1 ;;
    esac
    printf '%s/%s' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
    return 0
  fi
  if [[ "$url" =~ ^git@github\.com:(${owner})/(${repo})\.git$ ]]; then
    case "${BASH_REMATCH[2]}" in
      .|..) return 1 ;;
    esac
    printf '%s/%s' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
    return 0
  fi
  return 1
}
github_ssh_clone_url_for_repo() {
  printf 'git@github.com:%s.git' "$1"
}
password_store_origin() {
  pass git remote get-url origin 2>/dev/null || true
}
resolve_password_store_remote() {
  local current_origin current_repo
  current_origin="$(password_store_origin)"
  if [ -n "$current_origin" ]; then
    PASS_REPO="$(github_repo_from_clone_url "$current_origin")" \
      || die "password-store の既存 origin から GitHub repository を解決できません"
    PASS_HAS_ORIGIN=1
  else
    PASS_REPO="${GH_LOGIN}/password-store"
    PASS_HAS_ORIGIN=0
  fi
}
ensure_password_store_remote() {
  local current_origin current_repo
  current_origin="$(password_store_origin)"
  if [ -n "$current_origin" ]; then
    current_repo="$(github_repo_from_clone_url "$current_origin")" \
      || die "password-store の既存 origin から GitHub repository を解決できません"
    [ "$current_repo" = "$PASS_REPO" ] \
      || die "password-store の origin remote が登録対象 repository と矛盾しています。既存 origin は上書きしません"
  else
    [ "${REPO_EXISTS:-0}" -eq 0 ] \
      || die "password-store の origin remote が未設定です。既存 repository へ push する前に origin を手動設定してください"
  fi
}
remote_repo_from_name() {
  local remote="$1" remote_url
  remote_url="$(pass git remote get-url "$remote" 2>/dev/null)" \
    || return 1
  github_repo_from_clone_url "$remote_url"
}
verify_existing_upstream_push_target() {
  local upstream remote remote_repo
  upstream="$(pass git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null)" \
    || return 1
  remote="${upstream%%/*}"
  [ -n "$remote" ] && [ "$remote" != "$upstream" ] \
    || return 1
  remote_repo="$(remote_repo_from_name "$remote")" \
    || die "password-store Git repository の既存 upstream remote から GitHub repository を解決できません"
  [ "$remote_repo" = "$PASS_REPO" ] \
    || die "password-store Git repository の既存 upstream が登録対象 repository と一致しません。上書きせず停止します"
}

# ── 1. password-store recipient の GPG secret key 確認 / 未初期化時の pass init ──
PRIMARY_FINGERPRINT="$(ensure_password_store_initialized)"
[ -n "${PRIMARY_FINGERPRINT:-}" ] \
  || die "password-store 用 primary fingerprint を解決できません"
GPG_OWNER="$(gpg_unique_uid_for_fingerprint "$PRIMARY_FINGERPRINT")"
log "password-store recipient の秘密鍵を使用"
assert_eligible_gpg_primary "$PRIMARY_FINGERPRINT"
have_eligible_subkey_capability "$PRIMARY_FINGERPRINT" e || { log "encryption subkey を追加"; run_gpg_quick --quick-add-key "$GPG_OWNER" "$GPG_ALGO_ENCRYPT" encrypt never || die "GPG encryption subkey の追加に失敗しました"; }
have_eligible_subkey_capability "$PRIMARY_FINGERPRINT" a || { log "authentication subkey を追加"; run_gpg_quick --quick-add-key "$GPG_OWNER" "$GPG_ALGO_PRIMARY" auth never || die "GPG authentication subkey の追加に失敗しました"; }
have_eligible_subkey_capability "$PRIMARY_FINGERPRINT" s || { log "signing subkey を追加"; run_gpg_quick --quick-add-key "$GPG_OWNER" "$GPG_ALGO_PRIMARY" sign never || die "GPG signing subkey の追加に失敗しました"; }
log "GPG 鍵構成を確認済み"
warn "この鍵のバックアップと revocation certificate を別経路で保管してください。"

# ── 2. GitHub への SSH 公開鍵登録（authentication subkey 由来）──
log "authentication subkey 由来 SSH 公開鍵を GitHub に登録"
SSH_PUB="$(run_dotfiles gpg export-ssh-public-key)"
gh ssh-key list 2>/dev/null | grep -qF "$(printf '%s' "$SSH_PUB" | awk '{print $2}')" \
  || printf '%s\n' "$SSH_PUB" | gh ssh-key add - --title "dotfiles-gpg-auth-$(date +%Y%m%d)"

# ── 3. private password-store repository の remote 設定・push ──
resolve_password_store_remote
REPO_EXISTS=1
gh repo view "$PASS_REPO" >/dev/null 2>&1 || REPO_EXISTS=0
if [ "$REPO_EXISTS" -eq 1 ] && [ "${PASS_HAS_ORIGIN:-0}" -eq 0 ]; then
  die "password-store の origin remote が未設定です。既存 GitHub repository へ push する前に origin を手動設定してください"
fi
if [ "$REPO_EXISTS" -eq 0 ] && [ "${PASS_HAS_ORIGIN:-0}" -eq 1 ]; then
  log "GitHub に private password-store repository を作成"
  gh repo create "$PASS_REPO" --private --disable-issues --disable-wiki >/dev/null 2>&1 \
    || die "GitHub private password-store repository の作成に失敗しました"
  REPO_EXISTS=1
fi
if [ "$REPO_EXISTS" -eq 1 ]; then
  REPO_IS_PRIVATE="$(gh repo view "$PASS_REPO" --json isPrivate --jq .isPrivate 2>/dev/null)" \
    || die "GitHub password-store repository の visibility 確認に失敗しました"
  [ "$REPO_IS_PRIVATE" = "true" ] \
    || die "GitHub password-store repository が private ではありません。private repository を指定してください"
fi
PASSWORD_STORE_GIT_CREATED=0
if [ ! -d "$PASSWORD_STORE_ROOT/.git" ]; then
  log "password-store を Git repository として初期化して remote へ push"
  pass git init >/dev/null 2>&1 || true
  PASSWORD_STORE_GIT_CREATED=1
else
  log "設定済み password-store Git repository を使用"
fi
ensure_password_store_remote
pass git add -A >/dev/null 2>&1 || true
if pass git diff --cached --quiet >/dev/null 2>&1; then
  log "password-store に新規 commit 対象はありません"
else
  pass git commit -m "Initialize password-store" >/dev/null 2>&1 \
    || die "password-store Git commit に失敗しました。user.name/user.email/signing/hook 設定を確認してください"
fi
PUSH_COMPLETED=0
if [ "$REPO_EXISTS" -eq 0 ]; then
  log "GitHub に private password-store repository を作成して origin 設定と push を実行"
  gh repo create "$PASS_REPO" --private --disable-issues --disable-wiki --source "$PASSWORD_STORE_ROOT" --remote origin --push >/dev/null 2>&1 \
    || die "GitHub private password-store repository の作成または push に失敗しました"
  PUSH_COMPLETED=1
  REPO_EXISTS=1
fi
if [ "$PASSWORD_STORE_GIT_CREATED" -eq 1 ]; then
  pass git branch -M main >/dev/null 2>&1 || true
  if [ "$PUSH_COMPLETED" -eq 0 ]; then
    pass git push -u origin main >/dev/null 2>&1 \
      || die "password-store Git repository の push に失敗しました。remote 設定と GitHub SSH 認証を確認してください"
  fi
else
  CURRENT_BRANCH="$(pass git branch --show-current 2>/dev/null || true)"
  [ -n "$CURRENT_BRANCH" ] \
    || die "password-store Git repository の current branch を解決できません"
  if verify_existing_upstream_push_target; then
    pass git push >/dev/null 2>&1 \
      || die "password-store Git repository の push に失敗しました。既存 upstream と GitHub SSH 認証を確認してください"
  else
    pass git push -u origin "$CURRENT_BRANCH" >/dev/null 2>&1 \
      || die "password-store Git repository の push に失敗しました。remote 設定と GitHub SSH 認証を確認してください"
  fi
fi
confirm_password_store_primary_fingerprint

# ── 4. secret-recovery provisioning command を起動 ──
log "YubiKey へ bootstrap secret を登録"
run_dotfiles secrets yubikey enroll-primary

log "Bitwarden vault へ password-store-remote を登録または既存照合"
run_dotfiles secrets pass-remote register

warn "この script は gpg-secret-key-backup envelope を作成・投入・照合しません。enroll-spare の後に runbook の gate を満たし、dotfiles secrets gpg-backup register を実行してください。"

# ── 注意: 各サービスの YubiKey 物理登録 ──
warn "必要に応じて次を各サービスの UI / 管理画面で確認してください（script は途中停止しません）:
    - Bitwarden 個人 account: アカウント 2FA に primary/spare の YubiKey を登録。recovery code 保管。
    - GitHub: security key として primary/spare を登録。
    - Google / Apple 等: primary/spare を FIDO2/passkey 登録。recovery code 保管。"

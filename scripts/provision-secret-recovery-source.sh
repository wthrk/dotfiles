#!/usr/bin/env bash
#
# secret-recovery のソース（プロビジョニング元）環境での初期登録を補助するスクリプト。
#
# 既存 password-store の .gpg-id recipient とローカル GPG secret key を前提に、
# 利用者が事前作成した private password-store GitHub repository の確認・remote 設定/push、GitHub SSH 鍵登録、
# primary YubiKey への BWS access token 登録、primary から spare への再暗号化、
# BWS への復旧用 secret 登録、各 YubiKey の復旧前提検証を扱う。
# BWS project `dotfiles-secret-recovery` は事前に Bitwarden Secrets Manager 側で作成し、
# YubiKey に保存する token から 1 件だけ見える状態にしてからこの script の BWS 登録段階へ進む。
# YubiKey の serial は接続中のデバイスから自動検出する。明示指定する場合は
# `PROVISIONING_YUBIKEY_SERIAL` / `SPARE_YUBIKEY_SERIAL` 環境変数で指定する。
#
# 正本: docs/secret-recovery/initial-provisioning-runbook.md と spec/design。
# 使い方（実端末で。YubiKey touch の対話入力あり。source GPG key は passphrase-free であることを
# mutation 前の `gpg-backup validate` が検査する。BWS token 保存は子
# `enroll-primary` が controlling TTY から PIV 管理 PIN と token を hidden prompt で一度だけ読む。
# `enroll-spare` は primary から token を復号して spare に再暗号化するため token の再入力はない。
# PIN/token はこの script の stdin / argv / environment では扱わない）:
#   bash scripts/provision-secret-recovery-source.sh
#   bash scripts/provision-secret-recovery-source.sh --repo-head
#   bash scripts/provision-secret-recovery-source.sh --debug

set -euo pipefail
die() {
	printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2
	exit 1
}
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
USE_REPO_HEAD=0
PROVISION_DEBUG=0
PROVISIONING_YUBIKEY_SERIAL="${PROVISIONING_YUBIKEY_SERIAL:-}"
SPARE_YUBIKEY_SERIAL="${SPARE_YUBIKEY_SERIAL:-}"
case "${DOTFILES_PROVISION_USE_REPO_HEAD:-}" in
1) USE_REPO_HEAD=1 ;;
"") ;;
*) die "DOTFILES_PROVISION_USE_REPO_HEAD は 1 だけを受け付けます" ;;
esac
while [ "$#" -gt 0 ]; do
	case "$1" in
	--repo-head)
		USE_REPO_HEAD=1
		shift
		;;
	--debug)
		PROVISION_DEBUG=1
		shift
		;;
	--primary-serial)
		[ "$#" -ge 2 ] || die "--primary-serial には serial が必要です"
		PROVISIONING_YUBIKEY_SERIAL="$2"
		shift 2
		;;
	--spare-serial)
		[ "$#" -ge 2 ] || die "--spare-serial には serial が必要です"
		SPARE_YUBIKEY_SERIAL="$2"
		shift 2
		;;
	-h | --help)
		printf 'Usage: %s [--repo-head] [--debug] [--primary-serial SERIAL] [--spare-serial SERIAL]\n' "$0"
		exit 0
		;;
	*) die "未知の引数です: $1" ;;
	esac
done
# ─── 設定（既定値を自動導出。必要なら環境変数で上書き）───
GPG_ALGO_PRIMARY="${GPG_ALGO_PRIMARY:-ed25519}"
GPG_ALGO_ENCRYPT="${GPG_ALGO_ENCRYPT:-cv25519}"
PASS_REPO="${PASS_REPO:-}" # 例: <owner>/password-store。未指定なら GitHub ユーザーから導出
# `--primary-serial` / `--spare-serial` が同名 environment の既定値を上書きする。
# ──────────────────────────────────────────────────────

# すべての CLI 呼び出しは後段の `dotfiles` wrapper が controlling terminal を使う。
# 正本: docs/secret-recovery/initial-provisioning-runbook.md。Cargo の binary target 選択は
# https://doc.rust-lang.org/cargo/reference/cargo-targets.html#binaries を根拠に `--bin` で明示する。
# feature 専用の internal test stub ではなく、利用者向け production `dotfiles` を起動して
# プロビジョニングの実経路を選ぶためである。
run_dotfiles_from_repo_head() {
	(cd "$REPO_ROOT" && direnv exec . cargo run -p dotfiles-cli --bin dotfiles -- "$@")
}
log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[!]\033[0m %s\n' "$*"; }
pause() {
	printf '\n\033[1;35m[手動]\033[0m %s\n' "$*"
	IFS= read -r -p '    完了したら Enter: ' _ </dev/tty
}

preflight_github_ssh_key_scope() {
	gh api user/keys --paginate --jq 'length' >/dev/null ||
		die "GitHub SSH public key API の事前確認に失敗しました。gh の active account に admin:public_key scope が必要です: gh auth refresh -h github.com -s admin:public_key"
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
	local listing
	if ! listing="$(gpg --list-secret-keys --with-colons --fingerprint "$1")"; then
		die "GPG secret key の fingerprint 取得に失敗しました: $1"
	fi
	awk -F: '/^fpr:/{ print $10; exit }' <<<"$listing"
}
gpg_uid_for_fingerprint() {
	local listing
	if ! listing="$(gpg --list-secret-keys --with-colons "$1")"; then
		die "GPG secret key の UID 取得に失敗しました: $1"
	fi
	awk -F: '/^uid:/{ print $10; exit }' <<<"$listing"
}
verify_password_store_recipients() {
	local gpg_id_file
	gpg_id_file="$(password_store_gpg_id_file)"
	[ -s "$gpg_id_file" ] ||
		die "password-store が未初期化です（.gpg-id が存在しないか空です）"

	local primary_fingerprint=""
	local recipient fingerprint
	while IFS= read -r recipient; do
		[ -n "$recipient" ] || continue
		case "$recipient" in \#*) continue ;; esac
		fingerprint="$(recipient_secret_key_fingerprint "$recipient")"
		[ -n "$fingerprint" ] ||
			die "password-store recipient の秘密鍵がローカルにありません。import/generate が必要です: $recipient"
		if [ -z "$primary_fingerprint" ]; then
			primary_fingerprint="$fingerprint"
			continue
		fi
		[ "$primary_fingerprint" = "$fingerprint" ] ||
			die "password-store .gpg-id に異なる recipient が複数あります。primary fingerprint を一意に決められません"
	done <"$gpg_id_file"
	[ -n "$primary_fingerprint" ] ||
		die "password-store .gpg-id から有効な recipient を解決できません"
	printf '%s' "$primary_fingerprint"
}
confirm_password_store_primary_fingerprint() {
	local resolved_fingerprint
	resolved_fingerprint="$(verify_password_store_recipients)"
	[ -n "${resolved_fingerprint:-}" ] ||
		die "password-store .gpg-id から primary fingerprint を解決できません"
	[ "$resolved_fingerprint" = "$PRIMARY_FINGERPRINT" ] ||
		die "password-store .gpg-id の recipient が変更されています。解決済み primary fingerprint と一致しません"
	log "password-store .gpg-id recipient の秘密鍵を再確認済み"
}
enroll_primary_yubikey() {
	local serial="$1"
	local -a command=(secrets yubikey enroll-primary)
	[ "$PROVISION_DEBUG" -eq 0 ] || command+=(--debug)
	log "primary YubiKey を登録（BWS token はこの一回だけ hidden input）"
	if [ -n "$serial" ]; then
		command+=(--serial "$serial")
	fi
	dotfiles "${command[@]}"
}
enroll_spare_yubikey() {
	local primary_serial="$1"
	local spare_serial="$2"
	local -a command=(secrets yubikey enroll-spare
		--primary-serial "$primary_serial"
		--spare-serial "$spare_serial")
	[ "$PROVISION_DEBUG" -eq 0 ] || command+=(--debug)
	log "primary の BWS token を再入力せず spare YubiKey へ再暗号化して登録"
	dotfiles "${command[@]}"
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
		{ exec 9</dev/tty; } ||
			die "--repo-head の対話入力に必要な controlling terminal を開けません"
		local s
		if run_dotfiles_from_repo_head "$@" <&9; then
			s=0
		else
			s=$?
		fi
		exec 9<&-
		return "$s"
	}
else
	require dotfiles
fi

GH_LOGIN="$(gh api user --jq .login)" ||
	die "gh の active account で GitHub API 認証に失敗しました（gh auth login または gh auth switch）"
[ -n "$GH_LOGIN" ] ||
	die "gh の active account で GitHub API 認証に失敗しました（gh auth login または gh auth switch）"
preflight_github_ssh_key_scope

[ -z "${PASS_REPO:-}" ] && PASS_REPO="${GH_LOGIN}/password-store"
PASS_CLONE_URL="git@github.com:${PASS_REPO}.git"
PASSWORD_STORE_ROOT="$(password_store_dir)"
ensure_password_store_remote() {
	local current_origin status
	if current_origin="$(pass git config --get remote.origin.url)"; then
		[ "$current_origin" = "$PASS_CLONE_URL" ] ||
			die "password-store の origin remote が想定と異なります"
	else
		status=$?
		[ "$status" -eq 1 ] ||
			die "password-store の origin remote 確認に失敗しました"
		pass git remote add origin "$PASS_CLONE_URL"
	fi
}

# ── 1. password-store recipient の GPG secret key 確認 ──
PRIMARY_FINGERPRINT="$(verify_password_store_recipients)"
[ -n "${PRIMARY_FINGERPRINT:-}" ] ||
	die "password-store .gpg-id から primary fingerprint を解決できません"

GPG_OWNER="$(gpg_uid_for_fingerprint "$PRIMARY_FINGERPRINT")"
if [ -n "${GPG_OWNER:-}" ]; then
	log "password-store .gpg-id recipient の秘密鍵を使用: ${PRIMARY_FINGERPRINT} （${GPG_OWNER}）"
else
	log "password-store .gpg-id recipient の秘密鍵を使用: ${PRIMARY_FINGERPRINT}"
fi
have_cap() {
	local listing
	if ! listing="$(gpg --list-keys --with-colons "${PRIMARY_FINGERPRINT}")"; then
		die "GPG public key capability の取得に失敗しました"
	fi
	awk -F: -v c="$1" '/^sub:/{ if (index($12,c)) f=1 } END{ exit f?0:1 }' <<<"$listing"
}
have_cap e || {
	log "encryption subkey を追加"
	gpg --quick-add-key "${PRIMARY_FINGERPRINT}" "$GPG_ALGO_ENCRYPT" encrypt never
}
have_cap a || {
	log "authentication subkey を追加"
	gpg --quick-add-key "${PRIMARY_FINGERPRINT}" "$GPG_ALGO_PRIMARY" auth never
}
have_cap s || {
	log "signing subkey を追加"
	gpg --quick-add-key "${PRIMARY_FINGERPRINT}" "$GPG_ALGO_PRIMARY" sign never
}
# E/A/S の不足を補った後、GitHub/Git/YubiKey/BWS を変更する前に production と同じ gpgme export +
# Sequoia packet validator を非変更で通す。protected/unknown packet は pinentry を起動せずこの地点で停止する。
log "source GPG backup の passphrase-free E/A/S packet を非変更で検証"
dotfiles secrets gpg-backup validate --primary-fingerprint "$PRIMARY_FINGERPRINT"
log "GPG 鍵構成を確認済み"
warn "この鍵のバックアップと revocation certificate を別経路で保管してください。"
YUBIKEY_SERIAL="${PROVISIONING_YUBIKEY_SERIAL:-}"
SPARE_SERIAL="${SPARE_YUBIKEY_SERIAL:-}"
if [ -n "$SPARE_SERIAL" ] && [ -z "$YUBIKEY_SERIAL" ]; then
	die "SPARE_YUBIKEY_SERIAL を指定する場合は primary を一意に参照する PROVISIONING_YUBIKEY_SERIAL も指定してください"
fi

# ── 2. GitHub への SSH 公開鍵登録（authentication subkey 由来）──
log "authentication subkey 由来 SSH 公開鍵を GitHub に登録"
SSH_PUB="$(dotfiles gpg export-ssh-public-key --primary-fingerprint "${PRIMARY_FINGERPRINT}")"
SSH_KEY_BODY="$(printf '%s' "$SSH_PUB" | awk '{print $2}')"
[ -n "$SSH_KEY_BODY" ] || die "export した SSH 公開鍵を解釈できません"
if ! GH_SSH_KEYS="$(gh ssh-key list)"; then
	die "GitHub SSH public key 一覧の取得に失敗しました"
fi
if ! grep -qF "$SSH_KEY_BODY" <<<"$GH_SSH_KEYS"; then
	printf '%s\n' "$SSH_PUB" |
		gh ssh-key add - --title "dotfiles-gpg-auth-$(date +%Y%m%d)" ||
		die "GitHub SSH public key の登録に失敗しました"
fi

# ── 3. private password-store repository の remote 設定・push ──
# GitHub REST の repository 404 は private resource の不存在と認証/permission 不足を区別しない。
# script は 404 を create 許可へ変換せず、owner の private repository を人間が事前作成した後だけ進む。
pause "GitHub active account '${GH_LOGIN}' の owner 配下に private repository '${PASS_REPO}' を事前作成し、この account から閲覧・push できることを確認してください。この script は repository を作成しません。"
REPO_IS_PRIVATE="$(gh repo view "$PASS_REPO" --json isPrivate --jq .isPrivate)" ||
	die "GitHub password-store repository の存在・認証・permission・visibility を確認できません。repository を手動作成し、active account の権限を確認してください"
[ "$REPO_IS_PRIVATE" = "true" ] ||
	die "GitHub password-store repository が private ではありません。private repository を指定してください"
if [ ! -d "$PASSWORD_STORE_ROOT/.git" ]; then
	log "既存 password-store を Git repository として初期化して remote へ push"
	pass git init >/dev/null ||
		die "password-store Git repository の初期化に失敗しました"
	pass git branch -M main >/dev/null ||
		die "password-store Git repository の main branch 設定に失敗しました"
	PASS_PUSH_BRANCH="main"
	PASS_PUSH_MODE="set-upstream"
else
	log "既存 password-store Git repository を使用"
	PASS_PUSH_BRANCH="$(pass git branch --show-current)" ||
		die "既存 password-store repository の現在 branch の取得に失敗しました"
	[ -n "$PASS_PUSH_BRANCH" ] ||
		die "既存 password-store repository の現在 branch を解決できません。detached HEAD の場合は push 前に branch へ checkout してください"
	if PASS_UPSTREAM_REMOTE="$(pass git config --get "branch.${PASS_PUSH_BRANCH}.remote")"; then
		:
	else
		status=$?
		if [ "$status" -eq 1 ]; then
			PASS_UPSTREAM_REMOTE=""
		else
			die "password-store repository の upstream remote 設定確認に失敗しました"
		fi
	fi
	if PASS_UPSTREAM_MERGE="$(pass git config --get "branch.${PASS_PUSH_BRANCH}.merge")"; then
		:
	else
		status=$?
		if [ "$status" -eq 1 ]; then
			PASS_UPSTREAM_MERGE=""
		else
			die "password-store repository の upstream merge 設定確認に失敗しました"
		fi
	fi
	if [ -n "$PASS_UPSTREAM_REMOTE" ] && [ -n "$PASS_UPSTREAM_MERGE" ]; then
		PASS_UPSTREAM_MERGE_BRANCH="${PASS_UPSTREAM_MERGE#refs/heads/}"
		if pass git show-ref --verify --quiet "refs/remotes/${PASS_UPSTREAM_REMOTE}/${PASS_UPSTREAM_MERGE_BRANCH}" >/dev/null 2>&1; then
			PASS_PUSH_MODE="current-upstream"
		else
			PASS_PUSH_MODE="set-upstream"
		fi
	else
		PASS_PUSH_MODE="set-upstream"
	fi
fi
ensure_password_store_remote
pass git add -A >/dev/null ||
	die "password-store の Git index 更新に失敗しました"
if pass git diff --cached --quiet; then
	log "password-store に新規 commit 対象はありません"
else
	status=$?
	[ "$status" -eq 1 ] ||
		die "password-store の staged diff 確認に失敗しました"
	pass git commit -m "Initialize password-store" >/dev/null ||
		die "password-store Git commit に失敗しました。user.name/user.email/signing/hook 設定を確認してください"
fi
if [ "$PASS_PUSH_MODE" = "current-upstream" ]; then
	pass git push >/dev/null ||
		die "password-store Git repository の push に失敗しました。remote 設定と GitHub SSH 認証を確認してください"
else
	pass git push -u origin "$PASS_PUSH_BRANCH" >/dev/null ||
		die "password-store Git repository の push に失敗しました。remote 設定と GitHub SSH 認証を確認してください"
fi
confirm_password_store_primary_fingerprint

# ── 4. YubiKey への BWS access token 登録 ──
pause "Bitwarden Secrets Manager 側で次の gate をすべて確認してください:
    - project 'dotfiles-secret-recovery' が一意に 1 件だけ存在する
    - recovery 用 machine account を作成し、access token を発行済みである
    - machine account をその project に割り当て済みである
    - token に project の secret を read/create/update する権限がある
project / machine account / token の作成はこの script / dotfiles CLI では行いません。1 項目でも未確認なら Enter を押さず停止してください。"
enroll_primary_yubikey "$YUBIKEY_SERIAL"
if [ -n "${SPARE_SERIAL:-}" ]; then
	enroll_spare_yubikey "$YUBIKEY_SERIAL" "$SPARE_SERIAL"
fi

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

# ── 6. primary / spare の無対話復旧前提を検証 ──
log "primary YubiKey の無対話復旧前提を検証"
if [ -n "${YUBIKEY_SERIAL:-}" ]; then
	dotfiles secrets verify-yubikey --serial "$YUBIKEY_SERIAL" --all
else
	dotfiles secrets verify-yubikey --all
fi
if [ -n "${SPARE_SERIAL:-}" ]; then
	log "spare YubiKey の無対話復旧前提を検証"
	dotfiles secrets verify-yubikey --serial "$SPARE_SERIAL" --all
fi

# ── 手動: 各サービスの YubiKey 物理登録 ──
pause "次を各サービスの UI / 管理画面で行ってください（API でリモート登録できない物理/アカウント操作）:
    - Bitwarden Password Manager: アカウント 2FA に primary/spare の YubiKey を登録。recovery code 保管。
    - GitHub: security key として primary/spare を登録。
    - Google / Apple 等: primary/spare を FIDO2/passkey 登録。recovery code 保管。"

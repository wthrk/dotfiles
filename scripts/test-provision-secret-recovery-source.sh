#!/usr/bin/env bash
# `provision-secret-recovery-source.sh` の検証フローを fake gpg/pass/gh/dotfiles で実行する。
#
# 実 GitHub・GPG・password-store・personal vault へ触れず、既存 store / 新規 store / origin 正規化 /
# dotfiles CLI 呼び出し境界を一時ディレクトリ内の stub で固定する。
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
SCRIPT="$REPO_ROOT/scripts/provision-secret-recovery-source.sh"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

bin="$tmpdir/bin"
home="$tmpdir/home"
store="$tmpdir/password-store"
log="$tmpdir/fake.log"
mkdir -p "$bin" "$home" "$store"

cat >"$bin/gpg" <<'FAKE_GPG'
#!/usr/bin/env bash
set -euo pipefail

fp1="1111111111111111111111111111111111111111"
fp2="2222222222222222222222222222222222222222"

emit_key() {
  local fp="$1"
  local uid="Test User <test@example.invalid>"
  if [ "$fp" = "$fp2" ]; then
    uid="Other User <other@example.invalid>"
  fi
  if [ "${FAKE_GPG_DUPLICATE_UID:-0}" = "1" ]; then
    uid="Test User <test@example.invalid>"
  fi
  printf 'sec:u:255:22:%s:0:0:::::cC:\n' "${fp:0:16}"
  printf 'fpr:::::::::%s:\n' "$fp"
  printf 'uid:u:::::::0:%s:\n' "$uid"
  if [ "${FAKE_GPG_MISSING_SUBKEYS:-0}" != "1" ]; then
    printf 'ssb:u:255:22:%s:0:0:::::e:\n' "${fp:0:16}"
    printf 'ssb:u:255:22:%s:0:0:::::a:\n' "${fp:0:16}"
    printf 'ssb:u:255:22:%s:0:0:::::s:\n' "${fp:0:16}"
  fi
}

case " $* " in
  *" --quick-generate-key "*)
    printf 'gpg-generate:%s\n' "$*" >>"${FAKE_LOG:?}"
    printf 'gpg: revocation certificate stored for %s\n' "$fp1" >&2
    exit 0
    ;;
  *" --quick-add-key "*)
    printf 'gpg-quick-add-key:%s\n' "$*" >>"${FAKE_LOG:?}"
    printf 'gpg: added subkey for %s\n' "$fp1" >&2
    exit 0
    ;;
  *" --list-secret-keys "*)
    if [ "${FAKE_GPG_MODE:-single}" = "becomes-unusable" ]; then
      count_file="${FAKE_LOG:?}.gpg-list-count"
      count="$(cat "$count_file" 2>/dev/null || printf '0')"
      printf '%s\n' "$((count + 1))" >"$count_file"
      if [ "$count" -lt 4 ]; then
        emit_key "$fp1"
      fi
      exit 0
    fi
    if [ "${FAKE_GPG_MODE:-single}" = "none" ]; then
      if grep -q '^gpg-generate:' "${FAKE_LOG:?}" 2>/dev/null; then
        emit_key "$fp1"
      fi
    elif [ "${FAKE_GPG_MODE:-single}" = "multiple" ] && [ "${!#}" != "$fp1" ] && [ "${!#}" != "$fp2" ]; then
      emit_key "$fp1"
      emit_key "$fp2"
    else
      case "${!#}" in
        "$fp2"|*"$fp2"*|*"2222222222222222"*) emit_key "$fp2" ;;
        *) emit_key "$fp1" ;;
      esac
    fi
    ;;
esac
FAKE_GPG

cat >"$bin/pass" <<'FAKE_PASS'
#!/usr/bin/env bash
set -euo pipefail

store="${PASSWORD_STORE_DIR:?}"
log="${FAKE_LOG:?}"
cmd="${1:-}"
shift || true

case "$cmd" in
  init)
    mkdir -p "$store"
    printf '%s\n' "$1" >"$store/.gpg-id"
    printf 'pass-init:%s\n' "$1" >>"$log"
    ;;
  git)
    sub="${1:-}"
    shift || true
    case "$sub" in
      init) mkdir -p "$store/.git" ;;
      remote)
        case "${1:-}" in
          get-url)
            [ -s "$store/.git-origin" ] || exit 1
            cat "$store/.git-origin"
            ;;
          add)
            [ "${2:-}" = "origin" ] || exit 1
            printf '%s\n' "${3:-}" >"$store/.git-origin"
            printf 'git-remote-origin:%s\n' "${3:-}" >>"$log"
            ;;
        esac
        ;;
      add|commit) : ;;
      push)
        printf 'git-push:%s\n' "$*" >>"$log"
        ;;
      diff)
        [ "${1:-}" = "--cached" ] && [ "${2:-}" = "--quiet" ] && exit 0
        ;;
      branch)
        if [ "${1:-}" = "--show-current" ]; then
          printf 'main\n'
        fi
        ;;
      rev-parse)
        [ -n "${FAKE_GIT_UPSTREAM:-}" ] || exit 1
        printf '%s\n' "$FAKE_GIT_UPSTREAM"
        ;;
    esac
    ;;
esac
FAKE_PASS

cat >"$bin/gh" <<'FAKE_GH'
#!/usr/bin/env bash
set -euo pipefail

case "$1" in
  api)
    case "$2" in
      user) printf 'example-owner\n' ;;
      user/keys) printf '0\n' ;;
    esac
    ;;
  ssh-key)
    case "$2" in
      list) : ;;
      add) cat >/dev/null ;;
    esac
    ;;
  repo)
    case "$2" in
      view)
        if printf '%s\n' "$*" | grep -q -- '--json isPrivate'; then
          [ "${FAKE_REPO_EXISTS:-0}" = "1" ] || exit 1
          printf 'true\n'
          exit 0
        fi
        [ "${FAKE_REPO_EXISTS:-0}" = "1" ] || exit 1
        ;;
      create)
        printf 'gh-repo-create:%s\n' "$*" >>"${FAKE_LOG:?}"
        if printf '%s\n' "$*" | grep -q -- '--json isPrivate'; then
          printf 'true\n'
        fi
        ;;
    esac
    ;;
esac
FAKE_GH

cat >"$bin/git" <<'FAKE_GIT'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = "config" ]; then
  case "${3:-}" in
    user.name) printf 'Test User\n' ;;
    user.email) printf 'test@example.invalid\n' ;;
  esac
fi
FAKE_GIT

cat >"$bin/dotfiles" <<'FAKE_DOTFILES'
#!/usr/bin/env bash
set -euo pipefail
log="${FAKE_LOG:?}"
if [ "${1:-}" = "gpg" ] && [ "${2:-}" = "export-ssh-public-key" ]; then
  printf 'dotfiles:%s\n' "$*" >>"$log"
  printf 'ssh-ed25519 AAAATESTKEY fake@example.invalid\n'
  exit 0
fi
if env | grep -q 'git@github.com:example-owner/password-store.git'; then
  printf 'dotfiles-env-leaked-ssh-url\n' >>"$log"
fi
if IFS= read -r -t 0.1 stdin_line; then
  printf 'dotfiles-stdin:%s\n' "$stdin_line" >>"$log"
  cat >/dev/null || true
fi
printf 'dotfiles:%s\n' "$*" >>"$log"
FAKE_DOTFILES

cat >"$bin/direnv" <<'FAKE_DIRENV'
#!/usr/bin/env bash
set -euo pipefail
[ "${1:-}" = "exec" ] || exit 1
shift
[ "${1:-}" = "." ] || exit 1
shift
exec "$@"
FAKE_DIRENV

cat >"$bin/cargo" <<'FAKE_CARGO'
#!/usr/bin/env bash
set -euo pipefail
log="${FAKE_LOG:?}"
while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
  shift
done
[ "${1:-}" = "--" ] && shift
if [ "${1:-}" = "gpg" ] && [ "${2:-}" = "export-ssh-public-key" ]; then
  printf 'cargo-dotfiles:%s\n' "$*" >>"$log"
  printf 'ssh-ed25519 AAAATESTKEY fake@example.invalid\n'
  exit 0
fi
if env | grep -q 'git@github.com:example-owner/password-store.git'; then
  printf 'cargo-dotfiles-env-leaked-ssh-url\n' >>"$log"
fi
if IFS= read -r -t 0.1 stdin_line; then
  printf 'cargo-dotfiles-stdin:%s\n' "$stdin_line" >>"$log"
  cat >/dev/null || true
fi
printf 'cargo-dotfiles:%s\n' "$*" >>"$log"
FAKE_CARGO

chmod +x "$bin/gpg" "$bin/pass" "$bin/gh" "$bin/git" "$bin/dotfiles" "$bin/direnv" "$bin/cargo"

assert_dotfiles_order() {
  local enroll_line pass_line
  enroll_line="$(grep -n "^$1:secrets yubikey enroll-primary$" "$log" | cut -d: -f1)"
  pass_line="$(grep -n "^$1:secrets pass-remote register$" "$log" | cut -d: -f1)"
  [ -n "$enroll_line" ] && [ -n "$pass_line" ] \
    || return 1
  [ "$enroll_line" -lt "$pass_line" ]
}

assert_gpg_backup_register_not_run() {
  if grep -Eq "^$1:secrets gpg-backup register$" "$log"; then
    printf 'provisioning script must leave gpg-backup register to the post-enroll-spare gate\n' >&2
    exit 1
  fi
}

run_script() {
  local mode="$1"
  local scenario="${2:-new-store}"
  rm -f "$log" "${log}.gpg-list-count"
  rm -rf "$store"
  mkdir -p "$store"
  case "$scenario" in
    existing-repo)
      mkdir -p "$store/.git"
      printf '%s\n' '1111111111111111111111111111111111111111' >"$store/.gpg-id"
      printf '%s\n' 'git@github.com:example-owner/password-store.git' >"$store/.git-origin"
      ;;
    existing-https-origin)
      mkdir -p "$store/.git"
      printf '%s\n' '1111111111111111111111111111111111111111' >"$store/.gpg-id"
      printf '%s\n' 'https://github.com/example-owner/password-store.git' >"$store/.git-origin"
      ;;
    invalid-origin)
      mkdir -p "$store/.git"
      printf '%s\n' '1111111111111111111111111111111111111111' >"$store/.gpg-id"
      printf '%s\n' 'private-remote-value-must-not-leak' >"$store/.git-origin"
      ;;
  esac
  PATH="$bin:$PATH" \
    HOME="$home" \
    PASSWORD_STORE_DIR="$store" \
    FAKE_LOG="$log" \
    FAKE_REPO_EXISTS="$([ "$scenario" = "new-store" ] && printf 0 || printf 1)" \
    FAKE_GPG_MODE="$mode" \
    bash "$SCRIPT"
}

run_script_with_args() {
  local mode="$1"
  shift
  rm -f "$log"
  rm -rf "$store"
  mkdir -p "$store"
  PATH="$bin:$PATH" \
    HOME="$home" \
    PASSWORD_STORE_DIR="$store" \
    FAKE_LOG="$log" \
    FAKE_REPO_EXISTS=0 \
    FAKE_GPG_MODE="$mode" \
    bash "$SCRIPT" "$@"
}

run_script_with_pipe() {
  local mode="$1"
  rm -f "$log"
  rm -rf "$store"
  mkdir -p "$store"
  printf 'pipe-secret-that-must-not-reach-dotfiles\n' | PATH="$bin:$PATH" \
    HOME="$home" \
    PASSWORD_STORE_DIR="$store" \
    FAKE_LOG="$log" \
    FAKE_REPO_EXISTS=0 \
    FAKE_GPG_MODE="$mode" \
    bash "$SCRIPT"
}

run_script_duplicate_uid_existing_recipient() {
  rm -f "$log"
  rm -rf "$store"
  mkdir -p "$store"
  printf '%s\n' '1111111111111111111111111111111111111111' >"$store/.gpg-id"
  PATH="$bin:$PATH" \
    HOME="$home" \
    PASSWORD_STORE_DIR="$store" \
    FAKE_LOG="$log" \
    FAKE_REPO_EXISTS=0 \
    FAKE_GPG_MODE=multiple \
    FAKE_GPG_DUPLICATE_UID=1 \
    bash "$SCRIPT"
}

run_script_missing_subkeys() {
  rm -f "$log"
  rm -rf "$store"
  mkdir -p "$store"
  printf '%s\n' '1111111111111111111111111111111111111111' >"$store/.gpg-id"
  PATH="$bin:$PATH" \
    HOME="$home" \
    PASSWORD_STORE_DIR="$store" \
    FAKE_LOG="$log" \
    FAKE_REPO_EXISTS=0 \
    FAKE_GPG_MODE=single \
    FAKE_GPG_MISSING_SUBKEYS=1 \
    bash "$SCRIPT"
}

run_script_repo_head() {
  local mode="$1"
  rm -f "$log"
  rm -rf "$store"
  mkdir -p "$store"
  PATH="$bin:$PATH" \
    HOME="$home" \
    PASSWORD_STORE_DIR="$store" \
    FAKE_LOG="$log" \
    FAKE_REPO_EXISTS=0 \
    FAKE_GPG_MODE="$mode" \
    bash "$SCRIPT" --repo-head
}

run_script_repo_head_env() {
  local mode="$1"
  rm -f "$log"
  rm -rf "$store"
  mkdir -p "$store"
  PATH="$bin:$PATH" \
    HOME="$home" \
    PASSWORD_STORE_DIR="$store" \
    FAKE_LOG="$log" \
    FAKE_REPO_EXISTS=0 \
    FAKE_GPG_MODE="$mode" \
    DOTFILES_PROVISION_USE_REPO_HEAD=1 \
    bash "$SCRIPT"
}

run_script_with_repo_head_env_value() {
  local mode="$1"
  local env_value="$2"
  rm -f "$log"
  rm -rf "$store"
  mkdir -p "$store"
  PATH="$bin:$PATH" \
    HOME="$home" \
    PASSWORD_STORE_DIR="$store" \
    FAKE_LOG="$log" \
    FAKE_REPO_EXISTS=0 \
    FAKE_GPG_MODE="$mode" \
    DOTFILES_PROVISION_USE_REPO_HEAD="$env_value" \
    bash "$SCRIPT"
}

run_script single >"$tmpdir/single.out" 2>"$tmpdir/single.err"
grep -q "この script は gpg-secret-key-backup envelope を作成・投入・照合しません" "$tmpdir/single.out"
grep -q 'pass-init:Test User <test@example.invalid>' "$log"
grep -q '^dotfiles:gpg export-ssh-public-key$' "$log"
grep -q '^dotfiles:secrets pass-remote register$' "$log"
grep -q '^dotfiles:secrets yubikey enroll-primary$' "$log"
assert_gpg_backup_register_not_run dotfiles
assert_dotfiles_order dotfiles \
  || { printf 'provisioning commands must enroll YubiKey before password-store remote registration\n' >&2; exit 1; }
if grep -Eq '^(pass-init|gpg-quick-add-key):.*1111111111111111111111111111111111111111' "$log"; then
  printf 'provisioning script must not forward raw primary fingerprint through gpg/pass argv\n' >&2
  exit 1
fi
if grep -q '1111111111111111111111111111111111111111' "$tmpdir/single.out" "$tmpdir/single.err"; then
  printf 'provisioning script must not print raw primary fingerprint in logs or errors\n' >&2
  exit 1
fi
if grep -Eq '11111111|\.\.\.1111' "$tmpdir/single.out" "$tmpdir/single.err"; then
  printf 'provisioning script must not print primary fingerprint display fragments in logs or errors\n' >&2
  exit 1
fi
if grep -Eq -- '--url|--primary-fingerprint|--stdin' "$log"; then
  printf 'provisioning script must not forward input values or stdin mode through dotfiles argv\n' >&2
  exit 1
fi
if grep -Eq 'BWS|BW_SESSION|bw login|bw unlock|organization|project' "$tmpdir/single.out" "$tmpdir/single.err" "$log"; then
  printf 'provisioning script must not reintroduce BWS/bw CLI/session/project/organization flow\n' >&2
  exit 1
fi
if grep -q '^dotfiles-stdin:' "$log"; then
  printf 'provisioning script must not forward input values through dotfiles stdin\n' >&2
  exit 1
fi

run_script_with_pipe single >"$tmpdir/pipe.out" 2>"$tmpdir/pipe.err"
if grep -q '^dotfiles-stdin:' "$log"; then
  printf 'provisioning script must not forward piped script stdin through dotfiles stdin\n' >&2
  exit 1
fi

run_script_repo_head single >"$tmpdir/repo-head.out" 2>"$tmpdir/repo-head.err"
grep -q '^cargo-dotfiles:gpg export-ssh-public-key$' "$log"
grep -q '^cargo-dotfiles:secrets pass-remote register$' "$log"
grep -q '^cargo-dotfiles:secrets yubikey enroll-primary$' "$log"
assert_gpg_backup_register_not_run cargo-dotfiles
assert_dotfiles_order cargo-dotfiles \
  || { printf 'repo-head provisioning commands must enroll YubiKey before password-store remote registration\n' >&2; exit 1; }
if grep -q '^cargo-dotfiles-stdin:' "$log"; then
  printf 'repo-head dotfiles wrapper must not inherit script stdin\n' >&2
  exit 1
fi

run_script_repo_head_env single >"$tmpdir/repo-head-env.out" 2>"$tmpdir/repo-head-env.err"
grep -q '^cargo-dotfiles:gpg export-ssh-public-key$' "$log"
grep -q '^cargo-dotfiles:secrets pass-remote register$' "$log"
grep -q '^cargo-dotfiles:secrets yubikey enroll-primary$' "$log"
assert_gpg_backup_register_not_run cargo-dotfiles
assert_dotfiles_order cargo-dotfiles \
  || { printf 'repo-head env provisioning commands must enroll YubiKey before password-store remote registration\n' >&2; exit 1; }

if run_script_with_repo_head_env_value single 0 >"$tmpdir/repo-head-env-0.out" 2>"$tmpdir/repo-head-env-0.err"; then
  printf 'expected DOTFILES_PROVISION_USE_REPO_HEAD=0 to stop provisioning\n' >&2
  exit 1
fi
grep -q 'DOTFILES_PROVISION_USE_REPO_HEAD は 1 だけを受け付けます' "$tmpdir/repo-head-env-0.err"
if [ -f "$log" ] && grep -Eq '^(dotfiles|cargo-dotfiles):' "$log"; then
  printf 'DOTFILES_PROVISION_USE_REPO_HEAD=0 must stop before invoking dotfiles or cargo\n' >&2
  exit 1
fi

if run_script_with_repo_head_env_value single true >"$tmpdir/repo-head-env-true.out" 2>"$tmpdir/repo-head-env-true.err"; then
  printf 'expected DOTFILES_PROVISION_USE_REPO_HEAD=true to stop provisioning\n' >&2
  exit 1
fi
grep -q 'DOTFILES_PROVISION_USE_REPO_HEAD は 1 だけを受け付けます' "$tmpdir/repo-head-env-true.err"
if [ -f "$log" ] && grep -Eq '^(dotfiles|cargo-dotfiles):' "$log"; then
  printf 'DOTFILES_PROVISION_USE_REPO_HEAD=true must stop before invoking dotfiles or cargo\n' >&2
  exit 1
fi

if run_script becomes-unusable existing-repo >"$tmpdir/becomes-unusable.out" 2>"$tmpdir/becomes-unusable.err"; then
  printf 'expected unusable GPG key to stop provisioning\n' >&2
  exit 1
fi
grep -q 'GPG secret key が revoked / expired / disabled、またはローカルで使用不能です: \[redacted fingerprint\]' "$tmpdir/becomes-unusable.err"
if grep -q '1111111111111111111111111111111111111111' "$tmpdir/becomes-unusable.out" "$tmpdir/becomes-unusable.err"; then
  printf 'unusable-key error must not print raw primary fingerprint\n' >&2
  exit 1
fi
if grep -Eq '11111111|\.\.\.1111' "$tmpdir/becomes-unusable.out" "$tmpdir/becomes-unusable.err"; then
  printf 'unusable-key error must not print primary fingerprint display fragments\n' >&2
  exit 1
fi

run_script none >"$tmpdir/none.out" 2>"$tmpdir/none.err"
grep -q '^gpg-generate:--quick-generate-key Test User <test@example.invalid> ed25519 cert never$' "$log"
grep -q 'pass-init:Test User <test@example.invalid>' "$log"
if grep -Eq '^(pass-init|gpg-quick-add-key):.*1111111111111111111111111111111111111111' "$log"; then
  printf 'generated-key provisioning must not forward raw primary fingerprint through gpg/pass argv\n' >&2
  exit 1
fi
if grep -Eq '1111111111111111111111111111111111111111|11111111|\.\.\.1111' "$tmpdir/none.out" "$tmpdir/none.err"; then
  printf 'gpg quick-generate-key external stderr must not reach provisioning output\n' >&2
  exit 1
fi

run_script_missing_subkeys >"$tmpdir/missing-subkeys.out" 2>"$tmpdir/missing-subkeys.err"
grep -q '^gpg-quick-add-key:--quick-add-key Test User <test@example.invalid> cv25519 encrypt never$' "$log"
grep -q '^gpg-quick-add-key:--quick-add-key Test User <test@example.invalid> ed25519 auth never$' "$log"
grep -q '^gpg-quick-add-key:--quick-add-key Test User <test@example.invalid> ed25519 sign never$' "$log"
if grep -Eq '1111111111111111111111111111111111111111|11111111|\.\.\.1111' "$tmpdir/missing-subkeys.out" "$tmpdir/missing-subkeys.err"; then
  printf 'gpg quick-add-key external stderr must not reach provisioning output\n' >&2
  exit 1
fi

FAKE_GIT_UPSTREAM=main run_script single existing-repo >"$tmpdir/existing-repo.out" 2>"$tmpdir/existing-repo.err"
grep -q 'git-push:-u origin main' "$log"

FAKE_GIT_UPSTREAM=main run_script single existing-https-origin >"$tmpdir/existing-https-origin.out" 2>"$tmpdir/existing-https-origin.err"
grep -q 'git-push:-u origin main' "$log"
if grep -q 'git-remote-origin:' "$log"; then
  printf 'existing HTTPS password-store origin must be respected and not rewritten by the script\n' >&2
  exit 1
fi
if grep -Eq 'dotfiles-stdin:|dotfiles-env-leaked-ssh-url|--url|git@github.com:example-owner/password-store.git' "$log"; then
  printf 'script must not forward derived SSH clone URL through dotfiles argv, stdin, or env\n' >&2
  exit 1
fi

for scenario in bad-owner-origin extra-https-segment-origin extra-ssh-segment-origin; do
  case "$scenario" in
    bad-owner-origin)
      origin='https://github.com/bad_owner/repo.git'
      ;;
    extra-https-segment-origin)
      origin='https://github.com/owner/group/repo.git'
      ;;
    extra-ssh-segment-origin)
      origin='git@github.com:owner/group/repo.git'
      ;;
  esac
  rm -f "$log"
  rm -rf "$store"
  mkdir -p "$store/.git"
  printf '%s\n' '1111111111111111111111111111111111111111' >"$store/.gpg-id"
  printf '%s\n' "$origin" >"$store/.git-origin"
  if PATH="$bin:$PATH" HOME="$home" PASSWORD_STORE_DIR="$store" FAKE_LOG="$log" FAKE_GPG_MODE=single bash "$SCRIPT" >"$tmpdir/${scenario}.out" 2>"$tmpdir/${scenario}.err"; then
    printf 'expected invalid GitHub-like origin to stop provisioning for scenario: %s\n' "$scenario" >&2
    exit 1
  fi
  grep -q 'password-store の既存 origin から GitHub repository を解決できません' "$tmpdir/${scenario}.err"
  if grep -q "$origin" "$tmpdir/${scenario}.err"; then
    printf 'invalid origin rejection must not echo raw origin for scenario: %s\n' "$scenario" >&2
    exit 1
  fi
done

if run_script multiple >"$tmpdir/multiple.out" 2>"$tmpdir/multiple.err"; then
  printf 'expected multiple GPG keys to stop provisioning\n' >&2
  exit 1
fi
grep -q '複数の GPG secret key が存在します' "$tmpdir/multiple.err"
if [ -f "$log" ] && grep -q 'pass-init:' "$log"; then
  printf 'pass init must not run when multiple GPG keys are present\n' >&2
  exit 1
fi

if run_script_duplicate_uid_existing_recipient >"$tmpdir/duplicate-uid.out" 2>"$tmpdir/duplicate-uid.err"; then
  printf 'expected duplicate GPG UID to stop provisioning before UID-based quick-add-key\n' >&2
  exit 1
fi
grep -q '同一 UID を持つ GPG secret key が複数存在するため' "$tmpdir/duplicate-uid.err"
if [ -f "$log" ] && grep -q '^gpg-quick-add-key:' "$log"; then
  printf 'duplicate GPG UID must stop before gpg --quick-add-key\n' >&2
  exit 1
fi

if run_script_with_args single 'argv-secret-value-must-not-leak' >"$tmpdir/argv.out" 2>"$tmpdir/argv.err"; then
  printf 'expected unknown argv to stop provisioning\n' >&2
  exit 1
fi
grep -q '未知の引数です' "$tmpdir/argv.err"
if grep -q 'argv-secret-value-must-not-leak' "$tmpdir/argv.err"; then
  printf 'unknown argv rejection must not echo raw argv\n' >&2
  exit 1
fi

if run_script single invalid-origin >"$tmpdir/origin.out" 2>"$tmpdir/origin.err"; then
  printf 'expected invalid password-store origin to stop provisioning\n' >&2
  exit 1
fi
grep -q 'password-store の既存 origin から GitHub repository を解決できません' "$tmpdir/origin.err"
if grep -q 'private-remote-value-must-not-leak' "$tmpdir/origin.err"; then
  printf 'password-store origin errors must not echo private remote values\n' >&2
  exit 1
fi

printf 'provision-secret-recovery-source shell tests passed\n'

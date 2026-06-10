#!/usr/bin/env bash
# `provision-secret-recovery-source.sh` の検証フローを fake gpg/pass/gh/dotfiles で実行する。
#
# 実 GitHub・GPG・password-store・BWS へ触れず、既存 store / 新規 store / origin 正規化 /
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
  printf 'sec:u:255:22:%s:0:0:::::cC:\n' "${fp:0:16}"
  printf 'fpr:::::::::%s:\n' "$fp"
  printf 'uid:u:::::::0:Test User <%s@example.invalid>:\n' "$fp"
  printf 'ssb:u:255:22:%s:0:0:::::e:\n' "${fp:0:16}"
  printf 'ssb:u:255:22:%s:0:0:::::a:\n' "${fp:0:16}"
  printf 'ssb:u:255:22:%s:0:0:::::s:\n' "${fp:0:16}"
}

case " $* " in
  *" --quick-generate-key "*)
    printf 'gpg-generate:%s\n' "$*" >>"${FAKE_LOG:?}"
    exit 0
    ;;
  *" --quick-add-key "*)
    exit 0
    ;;
  *" --list-secret-keys "*)
    if [ "${FAKE_GPG_MODE:-single}" = "none" ]; then
      case "${!#}" in
        "$fp1"|*"$fp1"*|*"1111111111111111"*|*"Test User <test@example.invalid>"*) emit_key "$fp1" ;;
      esac
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
          printf 'true\n'
        fi
        ;;
      create) : ;;
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

run_script() {
  local mode="$1"
  local scenario="${2:-new-store}"
  rm -f "$log"
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
    FAKE_GPG_MODE="$mode" \
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
    FAKE_GPG_MODE="$mode" \
    DOTFILES_PROVISION_USE_REPO_HEAD="$env_value" \
    bash "$SCRIPT"
}

run_script single >"$tmpdir/single.out" 2>"$tmpdir/single.err"
grep -q 'pass-init:1111111111111111111111111111111111111111' "$log"
grep -q '^dotfiles:gpg export-ssh-public-key$' "$log"
grep -q '^dotfiles:secrets pass-remote register$' "$log"
grep -q '^dotfiles:secrets gpg-backup register$' "$log"
grep -q '^dotfiles:secrets yubikey put bws-access-token$' "$log"
if grep -q '1111111111111111111111111111111111111111' "$tmpdir/single.out" "$tmpdir/single.err"; then
  printf 'provisioning script must not print raw primary fingerprint in logs or errors\n' >&2
  exit 1
fi
if grep -Eq -- '--url|--primary-fingerprint|--stdin' "$log"; then
  printf 'provisioning script must not forward input values or stdin mode through dotfiles argv\n' >&2
  exit 1
fi
if grep -q '^dotfiles-stdin:' "$log"; then
  printf 'provisioning script must not forward input values through dotfiles stdin\n' >&2
  exit 1
fi

run_script_with_pipe single >/dev/null
if grep -q '^dotfiles-stdin:' "$log"; then
  printf 'provisioning script must not forward piped script stdin through dotfiles stdin\n' >&2
  exit 1
fi

run_script_repo_head single >/dev/null
grep -q '^cargo-dotfiles:gpg export-ssh-public-key$' "$log"
grep -q '^cargo-dotfiles:secrets pass-remote register$' "$log"
grep -q '^cargo-dotfiles:secrets gpg-backup register$' "$log"
grep -q '^cargo-dotfiles:secrets yubikey put bws-access-token$' "$log"
if grep -q '^cargo-dotfiles-stdin:' "$log"; then
  printf 'repo-head dotfiles wrapper must not inherit script stdin\n' >&2
  exit 1
fi

run_script_repo_head_env single >/dev/null
grep -q '^cargo-dotfiles:gpg export-ssh-public-key$' "$log"
grep -q '^cargo-dotfiles:secrets pass-remote register$' "$log"
grep -q '^cargo-dotfiles:secrets gpg-backup register$' "$log"
grep -q '^cargo-dotfiles:secrets yubikey put bws-access-token$' "$log"

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

run_script none >/dev/null
grep -q '^gpg-generate:--quick-generate-key Test User <test@example.invalid> ed25519 cert never$' "$log"
grep -q 'pass-init:1111111111111111111111111111111111111111' "$log"

FAKE_GIT_UPSTREAM=main run_script single existing-repo >/dev/null
grep -q 'git-push:-u origin main' "$log"

FAKE_GIT_UPSTREAM=main run_script single existing-https-origin >/dev/null
grep -q '^dotfiles:secrets pass-remote register$' "$log"
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
    printf 'expected invalid GitHub-like origin to stop provisioning: %s\n' "$origin" >&2
    exit 1
  fi
  grep -q 'password-store の既存 origin から GitHub repository を解決できません' "$tmpdir/${scenario}.err"
  if grep -q "$origin" "$tmpdir/${scenario}.err"; then
    printf 'invalid origin rejection must not echo raw origin: %s\n' "$origin" >&2
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

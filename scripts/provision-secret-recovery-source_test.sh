#!/usr/bin/env bash
#
# provision-secret-recovery-source.sh の BWS token 保存判定を、外部コマンドを起動せず検証する。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT

DOTFILES_PROVISION_SOURCE_ONLY=1 source "$REPO_ROOT/scripts/provision-secret-recovery-source.sh"

fail() {
  printf 'test failure: %s\n' "$*" >&2
  exit 1
}

reset_test_state() {
  unset BWS_ACCESS_TOKEN
  PUT_LOG="$TEST_DIR/put.log"
  : > "$PUT_LOG"
}

read_bws_access_token() {
  printf '%s' 'fixture-token'
}

test_saved_token_skips_prompt_and_put() {
  reset_test_state
  dotfiles() {
    if [ "$1 $2 $3" = 'secrets yubikey status' ]; then
      printf '%s\n' 'bitwarden-client-secret'
      return 0
    fi
    fail "保存済み token の確認で予期しない dotfiles 呼び出し: $*"
  }

  ensure_bws_access_token_stored primary 2001
  [ ! -s "$PUT_LOG" ] || fail '保存済み token に put が実行された'
  [ -z "${BWS_ACCESS_TOKEN:-}" ] || fail '保存済み token に prompt が実行された'
}

test_missing_token_prompts_and_puts() {
  reset_test_state
  dotfiles() {
    case "$1 $2 $3" in
      'secrets yubikey status')
        printf '%s\n' 'bw-email'
        ;;
      'secrets yubikey put')
        local stored_token
        IFS= read -r stored_token
        printf '%s|%s\n' "$*" "$stored_token" >> "$PUT_LOG"
        ;;
      *)
        fail "未保存 token の確認で予期しない dotfiles 呼び出し: $*"
        ;;
    esac
  }

  ensure_bws_access_token_stored spare 2002
  grep -Fxq 'secrets yubikey put bitwarden-client-secret --stdin --serial 2002|fixture-token' "$PUT_LOG" \
    || fail '未保存 token が prompt 後に対象 YubiKey へ保存されなかった'
}

test_status_failure_does_not_prompt_or_put() {
  reset_test_state
  dotfiles() {
    if [ "$1 $2 $3" = 'secrets yubikey status' ]; then
      return 42
    fi
    fail "status 失敗時に予期しない dotfiles 呼び出し: $*"
  }

  local output
  if output="$(ensure_bws_access_token_stored primary 2001 2>&1)"; then
    fail 'status 失敗を未保存として処理した'
  fi
  case "$output" in
    *'保存状況を確認できません'*) ;;
    *) fail 'status 失敗の停止理由が報告されない' ;;
  esac
  [ ! -s "$PUT_LOG" ] || fail 'status 失敗時に put が実行された'
}

test_saved_token_skips_prompt_and_put
test_missing_token_prompts_and_puts
test_status_failure_does_not_prompt_or_put

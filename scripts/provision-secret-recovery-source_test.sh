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

test_script_does_not_transport_yubikey_pin() {
  if grep -Eiq -- '--pin|PIV_PIN|YUBIKEY_PIN|pin_input' \
    "$REPO_ROOT/scripts/provision-secret-recovery-source.sh"; then
    fail 'provision script が YubiKey PIN を argv / environment / 独自 input として扱っている'
  fi
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

test_repeated_run_after_single_secret_put_does_not_clear() {
  reset_test_state
  local mutation_log="$TEST_DIR/repeated-run-mutation.log"
  local stored_marker="$TEST_DIR/repeated-run-stored"
  local initialized_marker="$TEST_DIR/repeated-run-initialized"
  : > "$mutation_log"
  rm -f "$stored_marker"
  rm -f "$initialized_marker"
  dotfiles() {
    case "$1 $2 $3" in
      'secrets yubikey status')
        if [ -e "$stored_marker" ]; then
          printf '%s\n' 'bitwarden-client-secret'
        fi
        ;;
      'secrets yubikey setup') printf '%s\n' "$*" >> "$mutation_log" ;;
      'secrets yubikey clear') fail "正常な partial storage に clear が実行された: $*" ;;
      'secrets yubikey put')
        if [ ! -e "$stored_marker" ] && [ ! -e "$initialized_marker" ]; then
          : > "$initialized_marker"
          return 43
        fi
        local stored_token
        IFS= read -r stored_token
        printf '%s|%s\n' "$*" "$stored_token" >> "$PUT_LOG"
        : > "$stored_marker"
        ;;
      *) fail "再実行時に予期しない dotfiles 呼び出し: $*" ;;
    esac
  }

  ensure_bws_access_token_stored primary 2001
  unset BWS_ACCESS_TOKEN
  ensure_bws_access_token_stored primary 2001
  grep -Fxq 'secrets yubikey setup --serial 2001' "$mutation_log" \
    || fail '初回の空 storage に setup が実行されなかった'
  [ "$(wc -l < "$mutation_log")" -eq 1 ] \
    || fail '再実行時に余分な storage mutation が実行された'
  [ "$(wc -l < "$PUT_LOG")" -eq 1 ] \
    || fail '再実行時に token の再入力または再保存が実行された'
}

test_empty_yubikey_initializes_before_put() {
  reset_test_state
  local setup_log="$TEST_DIR/setup.log"
  local initialized_marker="$TEST_DIR/empty-yubikey-initialized"
  : > "$setup_log"
  rm -f "$initialized_marker"
  dotfiles() {
    case "$1 $2 $3" in
      'secrets yubikey status') ;;
      'secrets yubikey setup') printf '%s\n' "$*" >> "$setup_log" ;;
      'secrets yubikey put')
        if [ ! -e "$initialized_marker" ]; then
          : > "$initialized_marker"
          return 43
        fi
        local stored_token
        IFS= read -r stored_token
        printf '%s|%s\n' "$*" "$stored_token" >> "$PUT_LOG"
        ;;
      *) fail "空 YubiKey で予期しない dotfiles 呼び出し: $*" ;;
    esac
  }

  ensure_bws_access_token_stored primary 2001
  grep -Fxq 'secrets yubikey setup --serial 2001' "$setup_log" \
    || fail '空 YubiKey に setup が実行されなかった'
  grep -Fxq 'secrets yubikey put bitwarden-client-secret --stdin --serial 2001|fixture-token' "$PUT_LOG" \
    || fail '空 YubiKey に token が保存されなかった'
}

test_normal_empty_manifest_puts_without_setup() {
  reset_test_state
  local mutation_log="$TEST_DIR/normal-empty-mutation.log"
  : > "$mutation_log"
  dotfiles() {
    case "$1 $2 $3" in
      'secrets yubikey status') ;;
      'secrets yubikey setup'|'secrets yubikey clear')
        printf '%s\n' "$*" >> "$mutation_log"
        ;;
      'secrets yubikey put')
        local stored_token
        IFS= read -r stored_token
        printf '%s|%s\n' "$*" "$stored_token" >> "$PUT_LOG"
        ;;
      *) fail "正常な空 manifest で予期しない dotfiles 呼び出し: $*" ;;
    esac
  }

  ensure_bws_access_token_stored primary 2001
  [ ! -s "$mutation_log" ] || fail '正常な空 manifest に setup/clear が実行された'
  grep -Fxq 'secrets yubikey put bitwarden-client-secret --stdin --serial 2001|fixture-token' "$PUT_LOG" \
    || fail '正常な空 manifest に token が保存されなかった'
}

test_put_failure_other_than_uninitialized_does_not_setup_or_retry() {
  reset_test_state
  local mutation_log="$TEST_DIR/put-failure-mutation.log"
  : > "$mutation_log"
  dotfiles() {
    case "$1 $2 $3" in
      'secrets yubikey status') ;;
      'secrets yubikey put') return 1 ;;
      'secrets yubikey setup'|'secrets yubikey clear')
        printf '%s\n' "$*" >> "$mutation_log"
        ;;
      *) fail "put の通常失敗時に予期しない dotfiles 呼び出し: $*" ;;
    esac
  }

  if (ensure_bws_access_token_stored primary 2001) >/dev/null 2>&1; then
    fail 'put の通常失敗後に成功扱いになった'
  fi
  [ ! -s "$mutation_log" ] || fail 'put の通常失敗後に setup/clear が実行された'
  [ ! -s "$PUT_LOG" ] || fail 'put の通常失敗後に保存済み token が記録された'
}

test_invalid_storage_clears_then_puts() {
  reset_test_state
  local mutation_log="$TEST_DIR/mutation.log"
  : > "$mutation_log"
  dotfiles() {
    case "$1 $2 $3" in
      'secrets yubikey status') return 42 ;;
      'secrets yubikey clear') printf '%s\n' "$*" >> "$mutation_log" ;;
      'secrets yubikey put')
        local stored_token
        IFS= read -r stored_token
        printf '%s|%s\n' "$*" "$stored_token" >> "$PUT_LOG"
        ;;
      *) fail "不正 storage で予期しない dotfiles 呼び出し: $*" ;;
    esac
  }

  ensure_bws_access_token_stored primary 2001
  grep -Fxq 'secrets yubikey clear --serial 2001 --yes' "$mutation_log" \
    || fail '不正 storage に clear が実行されなかった'
  ! grep -Fq 'secrets yubikey setup' "$mutation_log" \
    || fail 'clear 後の正常な空 manifest に setup が実行された'
}

test_clear_failure_does_not_prompt_or_put() {
  reset_test_state
  dotfiles() {
    case "$1 $2 $3" in
      'secrets yubikey status') return 42 ;;
      'secrets yubikey clear') return 42 ;;
      *) fail "clear 失敗時に予期しない dotfiles 呼び出し: $*" ;;
    esac
  }

  if (ensure_bws_access_token_stored primary 2001) >/dev/null 2>&1; then
    fail 'clear 失敗後に保存へ進んだ'
  fi
  [ ! -s "$PUT_LOG" ] || fail 'clear 失敗後に put が実行された'
}

test_unobservable_status_failure_does_not_clear_prompt_or_put() {
  reset_test_state
  dotfiles() {
    if [ "$1 $2 $3" = 'secrets yubikey status' ]; then
      return 1
    fi
    fail "観測不能な status 失敗時に予期しない dotfiles 呼び出し: $*"
  }

  if (ensure_bws_access_token_stored primary 2001) >/dev/null 2>&1; then
    fail '観測不能な status 失敗後に保存へ進んだ'
  fi
  [ ! -s "$PUT_LOG" ] || fail '観測不能な status 失敗後に put が実行された'
}

test_repo_head_retry_preserves_piped_token() {
  reset_test_state
  local event_log="$TEST_DIR/repo-head-events.log"
  local first_put_marker="$TEST_DIR/repo-head-first-put"
  : > "$event_log"
  rm -f "$first_put_marker"
  USE_REPO_HEAD=1
  dotfiles() {
    case "$1 $2 $3" in
      'secrets yubikey status') ;;
      'secrets yubikey setup') printf 'setup|0|%s\n' "$*" >> "$event_log" ;;
      *) fail "repo-head 再試行で予期しない通常 dotfiles 呼び出し: $*" ;;
    esac
  }
  run_dotfiles_from_repo_head() {
    [ "$1 $2 $3" = 'secrets yubikey put' ] \
      || fail "repo-head stdin 経路で予期しない呼び出し: $*"
    local stored_token
    IFS= read -r stored_token
    if [ ! -e "$first_put_marker" ]; then
      : > "$first_put_marker"
      printf 'put|43|%s|%s\n' "$*" "$stored_token" >> "$event_log"
      return 43
    fi
    printf 'put|0|%s|%s\n' "$*" "$stored_token" >> "$event_log"
  }

  ensure_bws_access_token_stored primary 2001
  local expected_events
  expected_events=$'put|43|secrets yubikey put bitwarden-client-secret --stdin --serial 2001|fixture-token\nsetup|0|secrets yubikey setup --serial 2001\nput|0|secrets yubikey put bitwarden-client-secret --stdin --serial 2001|fixture-token'
  [ "$(<"$event_log")" = "$expected_events" ] \
    || fail 'repo-head の再試行が put(43) → setup → 同じ stdin token の put(0) の順序になっていない'
  USE_REPO_HEAD=0
}

test_saved_token_skips_prompt_and_put
test_missing_token_prompts_and_puts
test_repeated_run_after_single_secret_put_does_not_clear
test_empty_yubikey_initializes_before_put
test_normal_empty_manifest_puts_without_setup
test_put_failure_other_than_uninitialized_does_not_setup_or_retry
test_invalid_storage_clears_then_puts
test_clear_failure_does_not_prompt_or_put
test_unobservable_status_failure_does_not_clear_prompt_or_put
test_repo_head_retry_preserves_piped_token
test_script_does_not_transport_yubikey_pin

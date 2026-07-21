#!/usr/bin/env bash
#
# provisioning source script が BWS token / PIV session の管理を Rust command へ委譲し、
# `status` / `clear` / `setup` / `put` を別 process として組み立てないことを検証する。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT

DOTFILES_PROVISION_SOURCE_ONLY=1 source "$REPO_ROOT/scripts/provision-secret-recovery-source.sh"

fail() {
  printf 'test failure: %s\n' "$*" >&2
  exit 1
}

test_script_does_not_transport_yubikey_pin_or_bws_token() {
  if grep -Eq -- '--pin|PIV_PIN|YUBIKEY_PIN|pin_input|BWS_ACCESS_TOKEN|read_bws_access_token' \
    "$REPO_ROOT/scripts/provision-secret-recovery-source.sh"; then
    fail 'provision script が PIV PIN または BWS token を argv / environment / 独自 input として扱っている'
  fi
}

test_script_has_no_split_yubikey_storage_transition() {
  if grep -Eq -- 'secrets yubikey (status|clear|setup|put)( |$)' \
    "$REPO_ROOT/scripts/provision-secret-recovery-source.sh"; then
    fail 'provision script が YubiKey storage transition を別 CLI process に分割している'
  fi
}

test_repo_head_selects_production_cli_binary() {
  local invocation_log="$TEST_DIR/repo-head-cargo-invocation.log"
  direnv() {
    printf '%s\n' "$*" > "$invocation_log"
  }

  run_dotfiles_from_repo_head gpg export-ssh-public-key --primary-fingerprint fixture-fingerprint
  grep -Fxq \
    'exec . cargo run -p dotfiles-cli --bin dotfiles -- gpg export-ssh-public-key --primary-fingerprint fixture-fingerprint' \
    "$invocation_log" \
    || fail '--repo-head は feature 専用 stub ではなく production dotfiles binary を明示しなければならない'
  unset -f direnv
}

test_primary_serial_uses_one_provision_command() {
  local invocation_log="$TEST_DIR/primary-invocation.log"
  dotfiles() {
    printf '%s\n' "$*" > "$invocation_log"
  }

  provision_bws_access_token_on_yubikey primary 2001
  [ "$(<"$invocation_log")" = 'secrets yubikey provision-bws-token --serial 2001' ] \
    || fail 'primary は serial 付き provision-bws-token 一回だけを呼ばなければならない'
}

test_implicit_serial_uses_one_provision_command() {
  local invocation_log="$TEST_DIR/implicit-invocation.log"
  dotfiles() {
    printf '%s\n' "$*" > "$invocation_log"
  }

  provision_bws_access_token_on_yubikey primary ''
  [ "$(<"$invocation_log")" = 'secrets yubikey provision-bws-token' ] \
    || fail 'serial 未指定は provision-bws-token 一回に委譲しなければならない'
}

test_script_does_not_transport_yubikey_pin_or_bws_token
test_script_has_no_split_yubikey_storage_transition
test_repo_head_selects_production_cli_binary
test_primary_serial_uses_one_provision_command
test_implicit_serial_uses_one_provision_command

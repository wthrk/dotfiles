#!/usr/bin/env bash
set -euo pipefail

pass=0
fail=0
skip=0

log_pass() { printf 'PASS %s\n' "$1"; pass=$((pass+1)); }
log_fail() { printf 'FAIL %s\n' "$1"; fail=$((fail+1)); }
log_skip() { printf 'SKIP %s\n' "$1"; skip=$((skip+1)); }

zrun() {
  local script="$1"
  POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true zsh -ic "$script" 2>/dev/null || true
}

expect_eq() {
  local id="$1" actual="$2" expected="$3"
  if [[ "$actual" == "$expected" ]]; then
    log_pass "$id"
  else
    log_fail "$id"
    printf '  expected: %s\n' "$expected"
    printf '  actual:   %s\n' "$actual"
  fi
}

expect_match() {
  local id="$1" actual="$2" regex="$3"
  if [[ "$actual" =~ $regex ]]; then
    log_pass "$id"
  else
    log_fail "$id"
    printf '  expected regex: %s\n' "$regex"
    printf '  actual:         %s\n' "$actual"
  fi
}

expect_eq "KEY:emacs:^I" "$(zrun "bindkey -M emacs '^I'")" '"^I" expand-or-complete'
expect_eq "KEY:viins:^I" "$(zrun "bindkey -M viins '^I'")" '"^I" expand-or-complete'
expect_eq "KEY:vicmd:^I" "$(zrun "bindkey -M vicmd '^I'")" '"^I" expand-or-complete'

fzf_tab_widget="$(zrun "zle -la | rg '^fzf-tab-complete$' | head -n 1")"
autosuggest_widget="$(zrun "zle -la | rg '^autosuggest-accept$' | head -n 1")"
syntax_fn="$(zrun "whence -w _zsh_highlight || whence -w _fast_highlight")"

if [[ -n "$fzf_tab_widget" ]]; then
  expect_eq "KEY:emacs:^X^I" "$(zrun "bindkey -M emacs '^X^I'")" '"^X^I" fzf-tab-complete'
  expect_match "WIDGET:fzf-tab-complete" "$fzf_tab_widget" '^fzf-tab-complete$'
else
  log_skip "KEY:emacs:^X^I"
  log_skip "WIDGET:fzf-tab-complete"
fi

if [[ -n "$autosuggest_widget" ]]; then
  expect_match "WIDGET:autosuggest-accept" "$autosuggest_widget" '^autosuggest-accept$'
else
  log_skip "WIDGET:autosuggest-accept"
fi

if [[ "$syntax_fn" =~ ^(_zsh_highlight|_fast_highlight):[[:space:]]function$ ]]; then
  log_pass "FUNC:syntax-highlighting"
else
  log_skip "FUNC:syntax-highlighting"
fi

path_dump="$(zrun 'print -l $path')"
if echo "$path_dump" | rg -n -q '(^|/)(\.nodebrew/current/bin|\.bun/bin|\.cargo/bin|\.pyenv/bin|\.rbenv/bin)$'; then
  log_fail "PATH:legacy-managers-absent"
else
  log_pass "PATH:legacy-managers-absent"
fi
expect_match "PATH:agent-tools-allowed" "$path_dump" '\.agent-tools/bin'
expect_match "PATH:rancher-desktop-allowed" "$path_dump" '\.rd/bin'

startup_err="$(script -q /dev/null zsh -ic 'exit' 2>&1 || true)"
if echo "$startup_err" | rg -q 'command not found|no such file|error'; then
  log_fail 'STARTUP:clean'
else
  log_pass 'STARTUP:clean'
fi

printf '\nSummary TOTAL=%d PASS=%d FAIL=%d SKIP=%d\n' "$((pass+fail+skip))" "$pass" "$fail" "$skip"
(( fail == 0 ))

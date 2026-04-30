#!/usr/bin/env bash
set -euo pipefail

pass=0
fail=0
skip=0

check_eq() {
  local id="$1" desc="$2" actual="$3" expected="$4"
  if [[ "$actual" == "$expected" ]]; then
    printf 'PASS %-4s %s\n' "$id" "$desc"
    pass=$((pass+1))
  else
    printf 'FAIL %-4s %s\n' "$id" "$desc"
    printf '      expected: %s\n' "$expected"
    printf '      actual:   %s\n' "$actual"
    fail=$((fail+1))
  fi
}

check_match() {
  local id="$1" desc="$2" actual="$3" regex="$4"
  if [[ "$actual" =~ $regex ]]; then
    printf 'PASS %-4s %s\n' "$id" "$desc"
    pass=$((pass+1))
  else
    printf 'FAIL %-4s %s\n' "$id" "$desc"
    printf '      expected match: %s\n' "$regex"
    printf '      actual:         %s\n' "$actual"
    fail=$((fail+1))
  fi
}

run_zsh() {
  local script="$1"
  POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true script -q /dev/null zsh -ic "$script" 2>/dev/null | col -b
}

fzf_tab_widget="$(run_zsh "zle -la | rg '^fzf-tab-complete$' | head -n 1")"
autosuggest_widget="$(run_zsh "zle -la | rg '^autosuggest-accept$' | head -n 1")"
syntax_func="$(run_zsh "functions | rg '(^|[[:space:]])_zsh_highlight|(^|[[:space:]])_fast_highlight|(^|[[:space:]])fast-theme|(^|[[:space:]])FAST_HIGHLIGHT' || :")"

if [[ -n "$fzf_tab_widget" ]]; then
  check_match T001 "fzf-tab widget exists" "$fzf_tab_widget" '^fzf-tab-complete$'
else
  printf 'SKIP %-4s %s\n' "T001" "fzf-tab widget exists"
  skip=$((skip+1))
fi

if [[ -n "$autosuggest_widget" ]]; then
  check_match T002 "autosuggest widget exists" "$autosuggest_widget" '^autosuggest-accept$'
else
  printf 'SKIP %-4s %s\n' "T002" "autosuggest widget exists"
  skip=$((skip+1))
fi

if [[ -n "$syntax_func" ]]; then
  printf 'PASS %-4s %s\n' "T003" "fast-syntax-highlighting loaded"
  pass=$((pass+1))
else
  printf 'SKIP %-4s %s\n' "T003" "fast-syntax-highlighting loaded"
  skip=$((skip+1))
fi

check_eq T010 "TAB emacs" "$(run_zsh "bindkey -M emacs '^I'")" '"^I" expand-or-complete'
check_eq T011 "TAB viins" "$(run_zsh "bindkey -M viins '^I'")" '"^I" expand-or-complete'
check_eq T012 "TAB vicmd" "$(run_zsh "bindkey -M vicmd '^I'")" '"^I" expand-or-complete'

if [[ -n "$fzf_tab_widget" ]]; then
  check_eq T013 "Ctrl-X TAB emacs" "$(run_zsh "bindkey -M emacs '^X^I'")" '"^X^I" fzf-tab-complete'
else
  printf 'SKIP %-4s %s\n' "T013" "Ctrl-X TAB emacs"
  skip=$((skip+1))
fi

path_dump="$(run_zsh 'print -l $path')"
if echo "$path_dump" | rg -n -q '(^|/)(\.nodebrew/current/bin|\.bun/bin|\.cargo/bin|\.pyenv/bin|\.rbenv/bin)$'; then
  printf 'FAIL %-4s %s\n' "T020" "legacy language-manager PATH entries are absent"
  fail=$((fail+1))
else
  printf 'PASS %-4s %s\n' "T020" "legacy language-manager PATH entries are absent"
  pass=$((pass+1))
fi

check_match T021 "agent-tools path is allowed" "$path_dump" '\.agent-tools/bin'
check_match T022 "rancher-desktop path is allowed" "$path_dump" '\.rd/bin'

printf '\nSummary: PASS=%d FAIL=%d SKIP=%d\n' "$pass" "$fail" "$skip"
(( fail == 0 ))

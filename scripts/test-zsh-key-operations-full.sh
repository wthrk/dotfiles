#!/usr/bin/env bash
set -euo pipefail

pass=0
fail=0
skip=0

log_pass() { printf 'PASS %s\n' "$1"; pass=$((pass+1)); }
log_fail() { printf 'FAIL %s\n' "$1"; fail=$((fail+1)); }
log_skip() { printf 'SKIP %s\n' "$1"; skip=$((skip+1)); }

zrun() {
  POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true zsh -ic "$1" 2>/dev/null
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

# 1) all keymaps exist and can dump bindings
map_list="$(zrun 'bindkey -l')"
for m in .safe command emacs isearch main vicmd viins viopp visual; do
  if grep -qx "$m" <<< "$map_list"; then
    log_pass "KMAP_PRESENT:$m"
  else
    log_fail "KMAP_PRESENT:$m"
  fi
  cnt="$(zrun "bindkey -M $m | wc -l | tr -d ' '")"
  if [[ "$cnt" =~ ^[0-9]+$ ]] && { (( cnt > 0 )) || [[ "$m" == "isearch" ]]; }; then
    log_pass "KMAP_DUMP:$m"
  else
    log_fail "KMAP_DUMP:$m"
    printf '  line_count: %s\n' "${cnt:-<empty>}"
  fi
done

# 2) expected features and key operations
plugins="$(cat "$HOME/.config/zsh/plugins.txt" 2>/dev/null || true)"
expect_eq "PLUGIN:p10k" "$(zrun 'whence -w p10k')" 'p10k: function'
expect_match "PLUGIN:autosuggestions" "$(zrun "zle -la | rg '^autosuggest-accept$' | head -n 1")" '^autosuggest-accept$'
expect_eq "PLUGIN:fast-syntax-highlighting" "$(zrun 'whence -w _zsh_highlight')" '_zsh_highlight: function'

expect_eq "KEY:viins:^P" "$(zrun "bindkey -M viins '^P'")" '"^P" history-beginning-search-backward-end'
expect_eq "KEY:viins:^N" "$(zrun "bindkey -M viins '^N'")" '"^N" history-beginning-search-forward-end'
expect_eq "KEY:viins:\\ep" "$(zrun "bindkey -M viins '\\ep'")" '"^[p" history-beginning-search-backward-end'
expect_eq "KEY:viins:\\en" "$(zrun "bindkey -M viins '\\en'")" '"^[n" history-beginning-search-forward-end'

if grep -q 'fzf-tab' <<< "$plugins"; then
  expect_eq "KEY:emacs:^I" "$(zrun "bindkey -M emacs '^I'")" '"^I" fzf-tab-complete'
  expect_eq "KEY:viins:^I" "$(zrun "bindkey -M viins '^I'")" '"^I" fzf-tab-complete'
  expect_eq "KEY:vicmd:^I" "$(zrun "bindkey -M vicmd '^I'")" '"^I" fzf-tab-complete'
else
  expect_eq "KEY:emacs:^I" "$(zrun "bindkey -M emacs '^I'")" '"^I" expand-or-complete'
  expect_eq "KEY:viins:^I" "$(zrun "bindkey -M viins '^I'")" '"^I" expand-or-complete'
  expect_eq "KEY:vicmd:^I" "$(zrun "bindkey -M vicmd '^I'")" '"^I" expand-or-complete'
fi

expect_eq "KEY:emacs:^T" "$(zrun "bindkey -M emacs '^T'")" '"^T" fzf-file-widget'
expect_eq "KEY:viins:^T" "$(zrun "bindkey -M viins '^T'")" '"^T" fzf-file-widget'
expect_eq "KEY:vicmd:^T" "$(zrun "bindkey -M vicmd '^T'")" '"^T" fzf-file-widget'

expect_eq "KEY:emacs:\\ec" "$(zrun "bindkey -M emacs '\\ec'")" '"^[c" fzf-cd-widget'
expect_eq "KEY:viins:\\ec" "$(zrun "bindkey -M viins '\\ec'")" '"^[c" fzf-cd-widget'
expect_eq "KEY:vicmd:\\ec" "$(zrun "bindkey -M vicmd '\\ec'")" '"^[c" fzf-cd-widget'

expect_eq "KEY:emacs:^R" "$(zrun "bindkey -M emacs '^R'")" '"^R" atuin-search'
expect_eq "KEY:viins:^R" "$(zrun "bindkey -M viins '^R'")" '"^R" atuin-search-viins'
expect_eq "KEY:vicmd:/" "$(zrun "bindkey -M vicmd '/'")" '"/" atuin-search'

expect_eq "FUNC:z" "$(zrun 'whence -w z')" 'z: function'
expect_eq "FUNC:zi" "$(zrun 'whence -w zi')" 'zi: function'
expect_eq "FUNC:_main_complete" "$(zrun 'whence -w _main_complete')" '_main_complete: function'
expect_eq "FUNC:_git" "$(zrun 'whence -w _git')" '_git: function'

if grep -q 'fzf-tab' <<< "$plugins"; then
  expect_eq "FUNC:fzf-tab-complete" "$(zrun 'whence -w fzf-tab-complete')" 'fzf-tab-complete: function'
  expect_eq "KEY:fzf-tab:emacs:^I" "$(zrun "bindkey -M emacs '^I'")" '"^I" fzf-tab-complete'
else
  log_skip "fzf-tab checks (plugin not installed)"
fi

if grep -q 'zsh-completions' <<< "$plugins"; then
  expect_match "FUNC:zsh-completions:fpath" "$(zrun "print -l \$fpath | rg 'zsh-completions/src$' | head -n 1")" 'zsh-completions/src$'
else
  log_skip "zsh-completions checks (plugin not installed)"
fi

startup_raw="$(script -q /dev/null zsh -ic 'exit' 2>&1 || true)"
startup_err="$(
  printf '%s\n' "$startup_raw" \
    | sed -E 's/\x1b\[[0-9;]*[A-Za-z]//g' \
    | sed -E 's/\x1b\][^\a]*\a//g' \
    | tr -d '\r\b' \
    | sed '/^[[:space:]]*$/d' \
    | sed '/^\^D*$/d'
)"
if [[ -z "$startup_err" ]]; then
  log_pass 'STARTUP:clean'
else
  log_fail 'STARTUP:clean'
  printf '  startup stderr (first lines):\n'
  printf '%s\n' "$startup_err" | sed -n '1,8p' | sed 's/^/  /'
fi

total=$((pass+fail+skip))
printf '\nSummary TOTAL=%d PASS=%d FAIL=%d SKIP=%d\n' "$total" "$pass" "$fail" "$skip"
(( fail == 0 ))

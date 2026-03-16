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

zrun_tty() {
  POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true \
    script -q /dev/null zsh -ic "$1" 2>/dev/null \
    | sed -E 's/\x1b\[[0-9;]*[A-Za-z]//g' \
    | sed -E 's/\x1b\][^\a]*\a//g' \
    | tr -d '\004\r\b' \
    | sed 's/^\^D//' \
    | sed '/^\^D*$/d'
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
expect_eq "PLUGIN:p10k:non-tty-gated" "$(zrun 'whence -w p10k')" 'p10k: none'
expect_eq "PLUGIN:autosuggestions:non-tty-gated" "$(zrun "zle -la | rg '^autosuggest-accept$' | head -n 1")" ''
expect_eq "PLUGIN:fast-syntax-highlighting:non-tty-gated" "$(zrun 'whence -w _zsh_highlight')" '_zsh_highlight: none'
expect_eq "PLUGIN:p10k:tty" "$(zrun_tty 'whence -w p10k')" 'p10k: function'
expect_match "PLUGIN:autosuggestions:tty" "$(zrun_tty "zle -la | rg '^autosuggest-accept$' | head -n 1")" '^autosuggest-accept$'
expect_eq "PLUGIN:fast-syntax-highlighting:tty" "$(zrun_tty 'whence -w _zsh_highlight')" '_zsh_highlight: function'

expect_eq "KEY:viins:^P" "$(zrun "bindkey -M viins '^P'")" '"^P" history-beginning-search-backward-end'
expect_eq "KEY:viins:^N" "$(zrun "bindkey -M viins '^N'")" '"^N" history-beginning-search-forward-end'
expect_eq "KEY:viins:\\ep" "$(zrun "bindkey -M viins '\\ep'")" '"^[p" history-beginning-search-backward-end'
expect_eq "KEY:viins:\\en" "$(zrun "bindkey -M viins '\\en'")" '"^[n" history-beginning-search-forward-end'

expect_eq "KEY:emacs:^I:non-tty" "$(zrun "bindkey -M emacs '^I'")" '"^I" expand-or-complete'
expect_eq "KEY:viins:^I:non-tty" "$(zrun "bindkey -M viins '^I'")" '"^I" expand-or-complete'
expect_eq "KEY:vicmd:^I:non-tty" "$(zrun "bindkey -M vicmd '^I'")" '"^I" expand-or-complete'
expect_eq "KEY:emacs:^I:tty" "$(zrun_tty "bindkey -M emacs '^I'")" '"^I" expand-or-complete'
expect_eq "KEY:viins:^I:tty" "$(zrun_tty "bindkey -M viins '^I'")" '"^I" expand-or-complete'
expect_eq "KEY:vicmd:^I:tty" "$(zrun_tty "bindkey -M vicmd '^I'")" '"^I" expand-or-complete'

expect_eq "KEY:emacs:^T:tty" "$(zrun_tty "bindkey -M emacs '^T'")" '"^T" fzf-file-widget'
expect_eq "KEY:viins:^T:tty" "$(zrun_tty "bindkey -M viins '^T'")" '"^T" fzf-file-widget'
expect_eq "KEY:vicmd:^T:tty" "$(zrun_tty "bindkey -M vicmd '^T'")" '"^T" fzf-file-widget'

expect_eq "KEY:emacs:\\ec:tty" "$(zrun_tty "bindkey -M emacs '\\ec'")" '"^[c" fzf-cd-widget'
expect_eq "KEY:viins:\\ec:tty" "$(zrun_tty "bindkey -M viins '\\ec'")" '"^[c" fzf-cd-widget'
expect_eq "KEY:vicmd:\\ec:tty" "$(zrun_tty "bindkey -M vicmd '\\ec'")" '"^[c" fzf-cd-widget'

expect_eq "KEY:emacs:^R:tty" "$(zrun_tty "bindkey -M emacs '^R'")" '"^R" atuin-search'
expect_eq "KEY:viins:^R:tty" "$(zrun_tty "bindkey -M viins '^R'")" '"^R" atuin-search-viins'
expect_eq "KEY:vicmd:/:tty" "$(zrun_tty "bindkey -M vicmd '/'")" '"/" atuin-search'

expect_eq "FUNC:z" "$(zrun 'whence -w z')" 'z: function'
expect_eq "FUNC:zi" "$(zrun 'whence -w zi')" 'zi: function'
expect_eq "FUNC:_main_complete" "$(zrun 'whence -w _main_complete')" '_main_complete: function'
expect_eq "FUNC:_git" "$(zrun 'whence -w _git')" '_git: function'

if grep -q 'fzf-tab' <<< "$plugins"; then
  expect_eq "FUNC:fzf-tab-complete:non-tty-gated" "$(zrun 'whence -w fzf-tab-complete')" 'fzf-tab-complete: none'
  expect_eq "FUNC:fzf-tab-complete:tty" "$(zrun_tty 'whence -w fzf-tab-complete')" 'fzf-tab-complete: function'
  expect_eq "KEY:fzf-tab:emacs:^X^I:tty" "$(zrun_tty "bindkey -M emacs '^X^I'")" '"^X^I" fzf-tab-complete'
else
  log_skip "fzf-tab checks (plugin not installed)"
fi

if grep -q 'zsh-completions' <<< "$plugins"; then
  expect_eq "FUNC:zsh-completions:fpath:non-tty-gated" "$(zrun "print -l \$fpath | rg 'zsh-completions/src$' | head -n 1")" ''
  expect_match "FUNC:zsh-completions:fpath:tty" "$(zrun_tty "print -l \$fpath | rg 'zsh-completions/src$' | head -n 1")" 'zsh-completions/src$'
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

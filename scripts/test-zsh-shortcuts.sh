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

mark_skip() {
  local id="$1" desc="$2" reason="$3"
  printf 'SKIP %-4s %s\n' "$id" "$desc"
  printf '      reason: %s\n' "$reason"
  skip=$((skip+1))
}

run_zsh() {
  local script="$1"
  POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true zsh -ic "${script}" 2>/dev/null
}

run_zsh_tty() {
  local script="$1"
  POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true \
    script -q /dev/null zsh -ic "${script}" 2>/dev/null \
    | sed -E 's/\x1b\[[0-9;]*[A-Za-z]//g' \
    | sed -E 's/\x1b\][^\a]*\a//g' \
    | tr -d '\004\r\b' \
    | sed 's/^\^D//' \
    | sed '/^\^D*$/d'
}

# --- Non-TTY startup: core shell remains usable, UI-heavy plugins stay gated ---
plugins="$(cat "$HOME/.config/zsh/plugins.txt")"
check_eq T001 "non-tty: powerlevel10k gated" "$(run_zsh "whence -w p10k")" 'p10k: none'
check_eq T002 "non-tty: autosuggestions gated" "$(run_zsh "zle -la | rg '^autosuggest-accept$' | head -n 1")" ''
check_eq T003 "non-tty: fast-syntax-highlighting gated" "$(run_zsh "whence -w _zsh_highlight")" '_zsh_highlight: none'
if [[ "$plugins" =~ fzf-tab ]]; then
  check_eq T004 "non-tty: fzf-tab gated" "$(run_zsh "whence -w fzf-tab-complete")" 'fzf-tab-complete: none'
else
  mark_skip T004 "plugin: fzf-tab enabled" "disabled in plugins.txt"
fi
if [[ "$plugins" =~ zsh-completions ]]; then
  check_eq T005 "non-tty: zsh-completions gated" "$(run_zsh "print -l \$fpath | rg 'zsh-completions/src$' | head -n 1")" ''
else
  mark_skip T005 "plugin: zsh-completions enabled" "disabled in plugins.txt"
fi

# --- Keymaps: TAB stays classic in all modes ---
check_eq T010 "non-tty TAB emacs" "$(run_zsh "bindkey -M emacs '^I'")" '"^I" expand-or-complete'
check_eq T011 "non-tty TAB viins" "$(run_zsh "bindkey -M viins '^I'")" '"^I" expand-or-complete'
check_eq T012 "non-tty TAB vicmd" "$(run_zsh "bindkey -M vicmd '^I'")" '"^I" expand-or-complete'

# --- Keymaps: custom history (works without TTY) ---
check_eq T020 "Ctrl-P viins" "$(run_zsh "bindkey -M viins '^P'")" '"^P" history-beginning-search-backward-end'
check_eq T021 "Ctrl-N viins" "$(run_zsh "bindkey -M viins '^N'")" '"^N" history-beginning-search-forward-end'
check_eq T022 "Esc-p viins" "$(run_zsh "bindkey -M viins '\\ep'")" '"^[p" history-beginning-search-backward-end'
check_eq T023 "Esc-n viins" "$(run_zsh "bindkey -M viins '\\en'")" '"^[n" history-beginning-search-forward-end'

# --- TTY startup: UI-heavy plugins and widgets are available ---
check_eq T030 "tty: powerlevel10k loaded" "$(run_zsh_tty "whence -w p10k")" 'p10k: function'
check_match T031 "tty: autosuggestions loaded" "$(run_zsh_tty "zle -la | rg '^autosuggest-accept$' | head -n 1")" '^autosuggest-accept$'
check_eq T032 "tty: fast-syntax-highlighting loaded" "$(run_zsh_tty "whence -w _zsh_highlight")" '_zsh_highlight: function'

if [[ "$plugins" =~ fzf-tab ]]; then
  check_eq T033 "tty: fzf-tab loaded" "$(run_zsh_tty "whence -w fzf-tab-complete")" 'fzf-tab-complete: function'
else
  mark_skip T033 "tty: fzf-tab loaded" "disabled in plugins.txt"
fi
if [[ "$plugins" =~ zsh-completions ]]; then
  check_match T034 "tty: zsh-completions loaded" "$(run_zsh_tty "print -l \$fpath | rg 'zsh-completions/src$' | head -n 1")" 'zsh-completions/src$'
else
  mark_skip T034 "tty: zsh-completions loaded" "disabled in plugins.txt"
fi

# --- Keymaps: TTY widgets ---
check_eq T040 "tty TAB emacs" "$(run_zsh_tty "bindkey -M emacs '^I'")" '"^I" expand-or-complete'
check_eq T041 "tty TAB viins" "$(run_zsh_tty "bindkey -M viins '^I'")" '"^I" expand-or-complete'
check_eq T042 "tty TAB vicmd" "$(run_zsh_tty "bindkey -M vicmd '^I'")" '"^I" expand-or-complete'
check_eq T043 "tty Ctrl-X TAB emacs" "$(run_zsh_tty "bindkey -M emacs '^X^I'")" '"^X^I" fzf-tab-complete'
check_eq T044 "tty Ctrl-T emacs" "$(run_zsh_tty "bindkey -M emacs '^T'")" '"^T" fzf-file-widget'
check_eq T045 "tty Ctrl-T viins" "$(run_zsh_tty "bindkey -M viins '^T'")" '"^T" fzf-file-widget'
check_eq T046 "tty Ctrl-T vicmd" "$(run_zsh_tty "bindkey -M vicmd '^T'")" '"^T" fzf-file-widget'
check_eq T047 "tty Esc-c emacs" "$(run_zsh_tty "bindkey -M emacs '\\ec'")" '"^[c" fzf-cd-widget'
check_eq T048 "tty Esc-c viins" "$(run_zsh_tty "bindkey -M viins '\\ec'")" '"^[c" fzf-cd-widget'
check_eq T049 "tty Esc-c vicmd" "$(run_zsh_tty "bindkey -M vicmd '\\ec'")" '"^[c" fzf-cd-widget'
check_eq T050 "tty Ctrl-R emacs -> atuin" "$(run_zsh_tty "bindkey -M emacs '^R'")" '"^R" atuin-search'
check_eq T051 "tty Ctrl-R viins -> atuin" "$(run_zsh_tty "bindkey -M viins '^R'")" '"^R" atuin-search-viins'
check_eq T052 "tty Ctrl-R vicmd -> fzf-history" "$(run_zsh_tty "bindkey -M vicmd '^R'")" '"^R" fzf-history-widget'
check_eq T053 "tty Slash vicmd -> atuin" "$(run_zsh_tty "bindkey -M vicmd '/'")" '"/" atuin-search'

# --- Widget/function presence ---
check_eq T054 "tty widget fzf-file" "$(run_zsh_tty "whence -w fzf-file-widget")" 'fzf-file-widget: function'
check_eq T055 "tty widget fzf-cd" "$(run_zsh_tty "whence -w fzf-cd-widget")" 'fzf-cd-widget: function'
check_eq T056 "tty widget fzf-history" "$(run_zsh_tty "whence -w fzf-history-widget")" 'fzf-history-widget: function'
check_match T057 "tty widget atuin-search" "$(run_zsh_tty "zle -la | rg '^atuin-search$' | head -n 1")" '^atuin-search$'
check_match T058 "tty widget atuin-search-viins" "$(run_zsh_tty "zle -la | rg '^atuin-search-viins$' | head -n 1")" '^atuin-search-viins$'
check_match T059 "tty autosuggest widget exists" "$(run_zsh_tty "zle -la | rg '^autosuggest-' | head -n 1")" '^autosuggest-'

if [[ "$plugins" =~ fzf-tab ]]; then
  check_eq T060 "tty widget fzf-tab-complete" "$(run_zsh_tty "whence -w fzf-tab-complete")" 'fzf-tab-complete: function'
else
  mark_skip T060 "tty widget fzf-tab-complete" "fzf-tab disabled"
fi

# --- Completion functions ---
check_eq T061 "core completion fn" "$(run_zsh "whence -w _main_complete")" '_main_complete: function'
check_eq T062 "git completion fn" "$(run_zsh "whence -w _git")" '_git: function'
check_eq T063 "brew completion fn" "$(run_zsh "whence -w _brew")" '_brew: function'

if [[ "$plugins" =~ zsh-completions ]]; then
  check_match T064 "tty zsh-completions fpath" "$(run_zsh_tty "print -l \$fpath | rg 'zsh-completions/src$' | head -n 1")" 'zsh-completions/src$'
else
  mark_skip T064 "extra completion certbot" "zsh-completions disabled"
fi

# --- zoxide ---
check_eq T070 "z function" "$(run_zsh "whence -w z")" 'z: function'
check_eq T071 "zi function" "$(run_zsh "whence -w zi")" 'zi: function'
check_eq T072 "zoxide completion helper" "$(run_zsh "whence -w __zoxide_z_complete")" '__zoxide_z_complete: function'

# --- setopt expectations ---
check_eq T080 "setopt correct" "$(run_zsh "setopt | rg -qx correct && echo on || echo off")" 'on'
check_eq T081 "setopt nolistbeep" "$(run_zsh "setopt | rg -qx nolistbeep && echo on || echo off")" 'on'
check_eq T082 "setopt auto_cd" "$(run_zsh "setopt | rg -qx autocd && echo on || echo off")" 'on'
check_eq T083 "setopt auto_pushd" "$(run_zsh "setopt | rg -qx autopushd && echo on || echo off")" 'on'
check_eq T084 "setopt complete_aliases" "$(run_zsh "setopt | rg -qx completealiases && echo on || echo off")" 'on'
check_eq T085 "setopt share_history" "$(run_zsh "setopt | rg -qx sharehistory && echo on || echo off")" 'on'
check_eq T086 "setopt inc_append_history" "$(run_zsh "setopt | rg -qx incappendhistory && echo on || echo off")" 'on'
check_eq T087 "setopt extended_history" "$(run_zsh "setopt | rg -qx extendedhistory && echo on || echo off")" 'on'
check_eq T088 "setopt hist_ignore_space" "$(run_zsh "setopt | rg -qx histignorespace && echo on || echo off")" 'on'

# --- startup health ---
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
  printf 'PASS %-4s %s\n' T090 "interactive startup clean"
  pass=$((pass+1))
else
  printf 'FAIL %-4s %s\n' T090 "interactive startup clean"
  printf '      got startup stderr (first lines):\n'
  printf '%s\n' "$startup_err" | sed -n '1,8p' | sed 's/^/      /'
  fail=$((fail+1))
fi

total=$((pass+fail+skip))
printf '\nSummary: TOTAL=%d PASS=%d FAIL=%d SKIP=%d\n' "$total" "$pass" "$fail" "$skip"
if (( fail > 0 )); then
  exit 1
fi

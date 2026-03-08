# Zsh Key Operations Test

## Test script
- `scripts/test-zsh-key-operations-full.sh`

## Coverage
- all runtime keymaps from `bindkey -l`
- plugin presence checks
- expected key operations for custom history / completion / fzf / atuin
- optional checks for `fzf-tab` and `zsh-completions`
- core functions (`z`, `zi`, `_main_complete`, `_git`)
- interactive startup stderr health check

## Last run summary
- TOTAL=44
- PASS=34
- FAIL=8
- SKIP=2

## Current failing groups
- `vicmd` TAB binding
- fzf key bindings (`Ctrl-T`, `Alt-c`) not applied
- interactive startup clean check (p10k/gitstatus + option errors)

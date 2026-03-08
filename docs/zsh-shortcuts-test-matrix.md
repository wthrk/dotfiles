# Zsh Shortcut Test Matrix

Date: 2026-03-08 (Asia/Tokyo)

This matrix covers all features that were agreed for the new configuration.

## Scope (expected features)
- Vi keymap (`bindkey -v`)
- Custom history search bindings (`Ctrl-P`, `Ctrl-N`, `Esc p`, `Esc n`)
- Completion on `Tab`
- `fzf` keybindings (`Ctrl-T`, `Esc c`, `Ctrl-R`)
- `atuin` history search bindings
- `zoxide` commands/completion
- `zsh-autosuggestions`
- `fast-syntax-highlighting`
- `powerlevel10k`
- Planned but currently disabled: `fzf-tab`, `zsh-completions`

## Test Items

| ID | Test item | Step | Expected | Actual | Result |
|---|---|---|---|---|---|
| T01 | Plugin set | `cat ~/.config/zsh/plugins.txt` | Includes active plugin list | `powerlevel10k`, `zsh-autosuggestions`, `fast-syntax-highlighting` | PASS |
| T02 | `Tab` in emacs map | `bindkey -M emacs '^I'` | Completion widget bound | `"^I" expand-or-complete` | PASS |
| T03 | `Tab` in vi insert map | `bindkey -M viins '^I'` | Completion widget bound | `"^I" expand-or-complete` | PASS |
| T04 | `Tab` in vi command map | `bindkey -M vicmd '^I'` | Completion widget bound (or intentionally mapped) | `"^I" undefined-key` | FAIL |
| T05 | Custom history backward | `bindkey -M viins '^P'` | `history-beginning-search-backward-end` | `"^P" history-beginning-search-backward-end` | PASS |
| T06 | Custom history forward | `bindkey -M viins '^N'` | `history-beginning-search-forward-end` | `"^N" history-beginning-search-forward-end` | PASS |
| T07 | `fzf` file widget | `bindkey -M emacs '^T'` | `fzf-file-widget` | `"^T" fzf-file-widget` | PASS |
| T08 | `fzf` cd widget | `bindkey -M emacs '\\ec'` | `fzf-cd-widget` | `"^[c" fzf-cd-widget` | PASS |
| T09 | `Ctrl-R` override (emacs) | `bindkey -M emacs '^R'` | `atuin-search` overrides fzf history | `"^R" atuin-search` | PASS |
| T10 | `Ctrl-R` override (vi insert) | `bindkey -M viins '^R'` | `atuin-search-viins` | `"^R" atuin-search-viins` | PASS |
| T11 | `Ctrl-R` in vi command mode | `bindkey -M vicmd '^R'` | `fzf-history-widget` (atuin does not override) | `"^R" fzf-history-widget` | PASS |
| T12 | Atuin widgets registered | `zle -la | rg '^atuin-'` | Atuin widgets exist | `atuin-search`, `atuin-search-viins`, etc. present | PASS |
| T13 | Zoxide commands | `whence -w z zi` | `z`, `zi` available | `z: function`, `zi: function` | PASS |
| T14 | Zoxide completion hook | `whence -w __zoxide_z_complete` | Hook exists | `__zoxide_z_complete: function` | PASS |
| T15 | Autosuggestions widgets | `zle -la | rg '^autosuggest-'` | Autosuggest widgets exist | multiple `autosuggest-*` widgets present | PASS |
| T16 | Syntax highlighting load | `cat plugins.txt` + runtime typing | plugin loaded and highlighting on input | plugin configured; runtime visual check required | PARTIAL |
| T17 | `fzf-tab` availability | `whence -w fzf-tab-complete` | widget exists if enabled | `fzf-tab-complete: none` | FAIL (disabled) |
| T18 | `zsh-completions` availability | `whence -w _certbot` | extra completion exists if enabled | `_certbot` currently available via system path; plugin itself disabled | PARTIAL |
| T19 | p10k startup health | `zsh -i -c exit` | no startup errors | `gitstatus failed to initialize`, `can't change option: monitor`, `can't change option: zle` | FAIL |

## Notes
- `T04` explains the bell on `Tab` in vi command mode.
- `T17` is expected to fail because `fzf-tab` is currently removed from `plugins.txt`.
- `T19` is a blocking stability issue; prompt initialization needs repair before further keybinding tuning.

## Raw commands used
```zsh
bindkey -M emacs '^I'
bindkey -M viins '^I'
bindkey -M vicmd '^I'
bindkey -M emacs '^R'
bindkey -M viins '^R'
bindkey -M vicmd '^R'
bindkey -M emacs '^T'
bindkey -M emacs '\ec'
zle -la | rg 'atuin|fzf|autosuggest'
whence -w z zi __zoxide_z_complete
whence -w fzf-tab-complete _certbot
zsh -i -c exit
```

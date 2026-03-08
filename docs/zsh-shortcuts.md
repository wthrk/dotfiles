# Zsh Expected Shortcuts and Key Operations

This document lists **all key/shortcut operations that are expected** for the agreed feature set,
and also marks whether each is currently active in this repo state.

## 1) Core editing mode (zsh)

- Vi mode enabled (`bindkey -v`)  
  Status: `ACTIVE`
- Insert mode -> command mode: `Esc`  
  Status: `ACTIVE`
- Command mode move: `h/j/k/l`, word motions, delete/change/yank family  
  Status: `ACTIVE`

## 2) Custom history search bindings (dotfiles)

- `Ctrl-P` -> `history-beginning-search-backward-end`
- `Ctrl-N` -> `history-beginning-search-forward-end`
- `Esc p` -> `history-beginning-search-backward-end`
- `Esc n` -> `history-beginning-search-forward-end`

Status: `ACTIVE`

## 3) Completion operations

### 3.1 Base completion (compinit)
- `Tab` -> complete current token/command/path
- Menu-style candidate selection (`zstyle ':completion:*' menu select`)

Status: `ACTIVE`

### 3.2 fzf-tab completion UI (planned)
- `Tab` on `cd`, `git checkout`, etc. should open fzf-based completion selector

Status: `NOT ACTIVE` (plugin currently removed from `config/zsh/plugins.txt`)

### 3.3 zsh-completions extended command coverage (planned)
- More command-specific completion definitions available via `Tab`

Status: `NOT ACTIVE` (plugin currently removed from `config/zsh/plugins.txt`)

## 4) fzf key bindings

From `fzf` key-bindings:
- `Ctrl-R` -> `fzf-history-widget` (if not overridden)
- `Ctrl-T` -> `fzf-file-widget`
- `Esc c` (Alt-c) -> `fzf-cd-widget`

Status:
- `Ctrl-T`: `ACTIVE`
- `Esc c`: `ACTIVE`
- `Ctrl-R`: `OVERRIDDEN by atuin in emacs/viins`

## 5) atuin history bindings

From `atuin init zsh --disable-up-arrow`:
- `Ctrl-R` (emacs) -> `atuin-search`
- `Ctrl-R` (vi insert) -> `atuin-search-viins`
- `/` (vi command mode) -> `atuin-search`

Status: `ACTIVE`

## 6) zoxide operations

Command-level operations (not primarily key-based):
- `z <query>` -> jump by frecency
- `zi <query>` -> interactive jump
- `z` completion hooks integrate with completion system

Status: `ACTIVE`

## 7) zsh-autosuggestions operations

Suggestion accept/partial accept is widget-based:
- Accept suggestion widgets: `forward-char`, `end-of-line`, `vi-forward-char`, `vi-end-of-line`, `vi-add-eol`
- Partial accept widgets: forward-word family (`forward-word`, `vi-forward-word`, etc.)

Typical key effect (depends on keymap):
- Right arrow / `Ctrl-F` / end-of-line motions accept suggestion
- Word-forward motions partially accept suggestion

Status: `ACTIVE`

## 8) fast-syntax-highlighting

- Real-time syntax highlighting while typing
- No dedicated keybinding expected

Status: `ACTIVE`

## 9) powerlevel10k

- Prompt rendering/theme only
- No mandatory shortcut expected for basic use
- `p10k configure` command for reconfiguration

Status: `ACTIVE` (but startup emitted gitstatus init errors in non-interactive checks)

## 10) Raw effective keymaps (ground truth)

Use these to verify current runtime mapping directly:

```zsh
bindkey -M emacs
bindkey -M viins
bindkey -M vicmd
```

(These are the definitive runtime bindings; plugin load order can alter them.)

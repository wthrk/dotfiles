autoload -Uz compinit

ZSH_CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/zsh"
ZSH_COMPDUMP="$ZSH_CACHE_DIR/.zcompdump-${ZSH_VERSION}"
mkdir -p "$ZSH_CACHE_DIR"

if [[ -f "$ZSH_COMPDUMP" ]]; then
  compinit -C -d "$ZSH_COMPDUMP"
else
  compinit -i -d "$ZSH_COMPDUMP"
fi

# Completion UI
zstyle ':completion:*' menu select
zstyle ':completion:*' list-colors "${(s.:.)LS_COLORS}"
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}'

# fzf-tab behavior
zstyle ':fzf-tab:*' fzf-command fzf
if [[ "$OSTYPE" == darwin* || "$OSTYPE" == freebsd* ]]; then
  zstyle ':fzf-tab:complete:cd:*' fzf-preview 'ls -la -G $realpath'
else
  zstyle ':fzf-tab:complete:cd:*' fzf-preview 'ls -la --color=always $realpath'
fi

# fzf shell key bindings (Ctrl-R/Ctrl-T/Alt-C) from Nix-provided fzf.
if [[ -o interactive ]] && [[ -t 0 ]] && [[ -t 1 ]] && (( $+commands[fzf] )); then
  fzf_root="$(cd "$(dirname -- "$(dirname -- "$(command -v fzf)")")" 2>/dev/null && pwd -P)"
  fzf_key_bindings="$fzf_root/share/fzf/key-bindings.zsh"
  if [[ -f "$fzf_key_bindings" ]]; then
    source "$fzf_key_bindings"
  fi
fi

# Keep classic TAB completion and expose fzf-tab on Ctrl-X TAB.
bindkey '^I' expand-or-complete
bindkey -M emacs '^I' expand-or-complete
bindkey -M viins '^I' expand-or-complete
bindkey -M vicmd '^I' expand-or-complete

if [[ -n ${widgets[fzf-tab-complete]} ]]; then
  bindkey '^X^I' fzf-tab-complete
  bindkey -M emacs '^X^I' fzf-tab-complete
  bindkey -M viins '^X^I' fzf-tab-complete
  bindkey -M vicmd '^X^I' fzf-tab-complete
fi

# External tools initialization
if [[ -o interactive ]] && [[ -t 0 ]] && [[ -t 1 ]] && (( $+commands[atuin] )); then
  eval "$(atuin init zsh --disable-up-arrow)"
fi

if [[ -o interactive ]] && (( $+commands[zoxide] )); then
  eval "$(zoxide init zsh)"
fi

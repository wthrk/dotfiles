# Shell behavior
setopt auto_cd
setopt auto_pushd
setopt correct
setopt list_packed
setopt noautoremoveslash
setopt nolistbeep
setopt complete_aliases

# Keybindings
bindkey -v

autoload -Uz history-search-end
zle -N history-beginning-search-backward-end history-search-end
zle -N history-beginning-search-forward-end history-search-end
bindkey '^p' history-beginning-search-backward-end
bindkey '^n' history-beginning-search-forward-end
bindkey '\ep' history-beginning-search-backward-end
bindkey '\en' history-beginning-search-forward-end

# Terminal title for common terminals
if [[ "$TERM" == xterm* || "$TERM" == screen* || "$TERM" == tmux* ]]; then
  precmd() {
    print -Pn "\e]0;%n@%m:%~\a"
  }
fi

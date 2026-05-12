# 対話操作の既定値を、補完候補が見やすく履歴移動が予測しやすい状態に揃える。
setopt auto_cd
setopt auto_pushd
setopt correct
setopt list_packed
setopt noautoremoveslash
setopt nolistbeep
setopt complete_aliases

# vi keymap を基準にしつつ、履歴検索は Ctrl-P/Ctrl-N と Alt-P/Alt-N で同じ関数へ寄せる。
bindkey -v

autoload -Uz history-search-end
zle -N history-beginning-search-backward-end history-search-end
zle -N history-beginning-search-forward-end history-search-end
bindkey '^p' history-beginning-search-backward-end
bindkey '^n' history-beginning-search-forward-end
bindkey '\ep' history-beginning-search-backward-end
bindkey '\en' history-beginning-search-forward-end

# 対応端末だけ、prompt 表示前に `user@host:path` をタイトルへ反映する。
autoload -Uz add-zsh-hook

if [[ "$TERM" == xterm* || "$TERM" == screen* || "$TERM" == tmux* ]]; then
  function _set_terminal_title() {
    print -Pn "\e]0;%n@%m:%~\a"
  }
  add-zsh-hook precmd _set_terminal_title
fi

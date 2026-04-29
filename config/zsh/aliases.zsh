alias where='command -v'
alias j='jobs -l'
alias du='du -h'
alias df='df -h'
alias su='su -l'

if (( $+commands[eza] )); then
  alias ls='eza --icons --git --group-directories-first'
  alias la='eza -a --icons --git --group-directories-first'
  alias lf='eza -F --icons --git --group-directories-first'
  alias ll='eza -lg --icons --git --group-directories-first'
elif [[ "$OSTYPE" == darwin* || "$OSTYPE" == freebsd* ]]; then
  alias ls='ls -G'
  alias la='ls -a'
  alias lf='ls -F'
  alias ll='ls -l'
else
  alias ls='ls --color=auto'
  alias la='ls -a'
  alias lf='ls -F'
  alias ll='ls -l'
fi

if (( $+commands[nvim] )); then
  alias vim='nvim'
  alias vi='nvim'
fi

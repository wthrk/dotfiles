alias where='command -v'
alias j='jobs -l'
alias la='ls -a'
alias lf='ls -F'
alias ll='ls -l'
alias du='du -h'
alias df='df -h'
alias su='su -l'

if [[ "$OSTYPE" == darwin* || "$OSTYPE" == freebsd* ]]; then
  alias ls='ls -G -w'
else
  alias ls='ls --color=auto'
fi

if (( $+commands[nvim] )); then
  alias vim='nvim'
  alias vi='nvim'
fi

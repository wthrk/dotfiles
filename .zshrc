## Environment variable configuration
#
# LANG
# export LANG=ja_JP.UTF-8

## Default shell configuration
#
# set prompt
#

if [ "$(uname)" = "Darwin" ]; then
  if [ "$(arch)" = "i386" ]; then
    export PROMPT_ARCH="(x86_64)"
  fi
fi

autoload colors
colors
case ${UID} in
0)
  PROMPT="%B%{${fg[red]}%}${PROMPT_ARCH}%/#%{${reset_color}%}%b "
  PROMPT2="%B%{${fg[red]}%}%_#%{${reset_color}%}%b "
  SPROMPT="%B%{${fg[red]}%}%r is correct? [n,y,a,e]:%{${reset_color}%}%b "
  [ -n "${REMOTEHOST}${SSH_CONNECTION}" ] &&
    PROMPT="%{${fg[white]}%}${HOST%%.*} ${PROMPT}"
  ;;
*)
  PROMPT="%{${fg[red]}%}${PROMPT_ARCH}%/%%%{${reset_color}%} "
  PROMPT2="%{${fg[red]}%}%_%%%{${reset_color}%} "
  SPROMPT="%{${fg[red]}%}%r is correct? [n,y,a,e]:%{${reset_color}%} "
  [ -n "${REMOTEHOST}${SSH_CONNECTION}" ] &&
    PROMPT="%{${fg[white]}%}${HOST%%.*} ${PROMPT}"
  ;;
esac

# auto change directory
#
setopt auto_cd

# auto directory pushd that you can get dirs list by cd -[tab]
#
setopt auto_pushd

# command correct edition before each completion attempt
#
setopt correct

# compacked complete list display
#
setopt list_packed

# no remove postfix slash of command line
#
setopt noautoremoveslash

# no beep sound when complete list displayed
#
setopt nolistbeep

## Keybind configuration
#
# emacs like keybind (e.x. Ctrl-a goes to head of a line and Ctrl-e goes
# to end of it)
#
bindkey -v

# historical backward/forward search with linehead string binded to ^P/^N
#
autoload history-search-end
zle -N history-beginning-search-backward-end history-search-end
zle -N history-beginning-search-forward-end history-search-end
bindkey "^p" history-beginning-search-backward-end
bindkey "^n" history-beginning-search-forward-end
bindkey "\\ep" history-beginning-search-backward-end
bindkey "\\en" history-beginning-search-forward-end

## Command history configuration
#
HISTFILE=~/.zsh_history
HISTSIZE=10000
SAVEHIST=10000
setopt hist_ignore_dups # ignore duplication command history list
setopt share_history # share command history data
setopt HIST_IGNORE_SPACE

## Alias configuration
#
# expand aliases before completing
#
setopt complete_aliases # aliased ls needs if file/dir completions work

alias where="command -v"
alias j="jobs -l"

case "${OSTYPE}" in
freebsd*|darwin*)
  alias ls="ls -G -w"
  ;;
linux*)
  alias ls="ls --color"
  ;;
esac

alias la="ls -a"
alias lf="ls -F"
alias ll="ls -l"
alias du="du -h"
alias df="df -h"
alias su="su -l"

alias vim="nvim"
alias vi="nvim"

## terminal configuration
#
unset LSCOLORS
case "${TERM}" in
xterm)
  export TERM=xterm-color
  ;;
kterm)
  export TERM=kterm-color
  # set BackSpace control character
  stty erase
  ;;
cons25)
  unset LANG
  export LSCOLORS=ExFxCxdxBxegedabagacad
  export LS_COLORS='di=01;34:ln=01;35:so=01;32:ex=01;31:bd=46;34:cd=43;34:su=41;30:sg=46;30:tw=42;30:ow=43;30'
  zstyle ':completion:*' list-colors \
    'di=;34;1' 'ln=;35;1' 'so=;32;1' 'ex=31;1' 'bd=46;34' 'cd=43;34'
  ;;
esac

# set terminal title including current directory
#
case "${TERM}" in
kterm*|xterm*)
  precmd() {
    echo -ne "\033]0;${USER}@${HOST%%.*}:${PWD}\007"
  }
  export LSCOLORS=exfxcxdxbxegedabagacad
  export LS_COLORS='di=34:ln=35:so=32:pi=33:ex=31:bd=46;34:cd=43;34:su=41;30:sg=46;30:tw=42;30:ow=43;30'
  zstyle ':completion:*' list-colors \
    'di=34' 'ln=35' 'so=32' 'ex=31' 'bd=46;34' 'cd=43;34'
  ;;
esac

# env
export EDITOR=nvim
export XDG_CONFIG_HOME=~/.config

## Completion configuration
#
autoload -U compinit
compinit

if [ "$(uname)" = "Darwin" ]; then
  # switch arm64/x86_64
  if (( $+commands[arch] )); then
    alias a64="exec arch -arch arm64e /opt/homebrew/bin/zsh"
    alias x64="exec arch -arch x86_64 /usr/local/bin/zsh"
  fi

  function runs_on_ARM64() { [[ `uname -m` = "arm64" ]]; }
  function runs_on_X86_64() { [[ `uname -m` = "x86_64" ]]; }

  BREW_PATH_OPT="/opt/homebrew/bin"
  BREW_PATH_LOCAL="/usr/local/bin"
  function brew_exists_at_opt() { [[ -d ${BREW_PATH_OPT} ]]; }
  function brew_exists_at_local() { [[ -d ${BREW_PATH_LOCAL} ]]; }

  setopt no_global_rcs
  typeset -U path PATH
  path=($path /usr/sbin /sbin)

  if runs_on_ARM64; then
    path=($BREW_PATH_OPT(N-/) $BREW_PATH_LOCAL(N-/) $path)
  else
    path=($BREW_PATH_LOCAL(N-/) $path)
  fi
fi

## load user .zshrc configuration file
#
[ -f ~/.zshrc.mine ] && source ~/.zshrc.mine

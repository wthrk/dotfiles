# 管理対象の対話シェルで、重複を抑えつつ複数端末間で履歴を共有する。
HISTFILE="$HOME/.zsh_history"
HISTSIZE=50000
SAVEHIST=50000

setopt hist_ignore_dups
setopt hist_ignore_all_dups
setopt hist_reduce_blanks
setopt hist_verify
setopt hist_expire_dups_first
setopt share_history
setopt inc_append_history
setopt extended_history
setopt hist_ignore_space

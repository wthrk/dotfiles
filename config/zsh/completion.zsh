# 対話 zsh で使う補完システムを初期化する。
autoload -Uz compinit

ZSH_CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/zsh"
ZSH_COMPDUMP="$ZSH_CACHE_DIR/.zcompdump-${ZSH_VERSION}"
mkdir -p "$ZSH_CACHE_DIR"

# `-C` にすると、system 層の compinit より後に home 層が fpath へ足したディレクトリを監査する経路が
# 無くなる。`-i` は監査したうえで落ちたものを fpath から外して続行する。
compinit -i -d "$ZSH_COMPDUMP"

# 補完候補の表示は zsh 標準補完に合わせ、曖昧な候補を選びやすくする。
zstyle ':completion:*' menu select
zstyle ':completion:*' list-colors "${(s.:.)LS_COLORS}"
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}'

# fzf-tab は補完候補の絞り込みにだけ使い、通常の TAB 動作は奪わない。
zstyle ':fzf-tab:*' fzf-command fzf
if [[ "$OSTYPE" == darwin* || "$OSTYPE" == freebsd* ]]; then
  zstyle ':fzf-tab:complete:cd:*' fzf-preview 'ls -la -G $realpath'
else
  zstyle ':fzf-tab:complete:cd:*' fzf-preview 'ls -la --color=always $realpath'
fi

# Nix が提供する fzf の Ctrl-R/Ctrl-T/Alt-C 設定を読み込む。
if [[ -o interactive ]] && [[ -t 0 ]] && [[ -t 1 ]] && (( $+commands[fzf] )); then
  fzf_root="$(cd "$(dirname -- "$(dirname -- "$(command -v fzf)")")" 2>/dev/null && pwd -P)"
  fzf_key_bindings="$fzf_root/share/fzf/key-bindings.zsh"
  if [[ -f "$fzf_key_bindings" ]]; then
    source "$fzf_key_bindings"
  fi
fi

# TAB は zsh 標準補完、Ctrl-X TAB は fzf-tab に分けて、既存の筋肉記憶を壊さない。
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

# 外部ツールの補完設定は、コマンドが存在する場合だけ読み込む。
if [[ -o interactive ]] && [[ -t 0 ]] && [[ -t 1 ]] && (( $+commands[atuin] )); then
  eval "$(atuin init zsh --disable-up-arrow)"
fi

if [[ -o interactive ]] && (( $+commands[zoxide] )); then
  eval "$(zoxide init zsh)"
fi

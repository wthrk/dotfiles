# Antidote plugin manager
ANTIDOTE_HOME="${XDG_DATA_HOME:-$HOME/.local/share}/antidote"
ANTIDOTE_SCRIPT="$ANTIDOTE_HOME/antidote.zsh"
ANTIDOTE_BUNDLE="${XDG_CACHE_HOME:-$HOME/.cache}/zsh/plugins.zsh"
PLUGINS_FILE="$HOME/.config/zsh/plugins.txt"

mkdir -p "${XDG_CACHE_HOME:-$HOME/.cache}/zsh"

if [[ ! -f "$ANTIDOTE_SCRIPT" ]] && (( $+commands[git] )); then
  command git clone --depth=1 https://github.com/mattmc3/antidote.git "$ANTIDOTE_HOME" >/dev/null 2>&1
fi

if [[ -f "$ANTIDOTE_SCRIPT" && -f "$PLUGINS_FILE" ]]; then
  if [[ ! -f "$ANTIDOTE_BUNDLE" || "$PLUGINS_FILE" -nt "$ANTIDOTE_BUNDLE" ]]; then
    (
      source "$ANTIDOTE_SCRIPT"
      antidote bundle <"$PLUGINS_FILE" >| "$ANTIDOTE_BUNDLE"
    )
  fi

  if [[ -f "$ANTIDOTE_BUNDLE" ]]; then
    source "$ANTIDOTE_BUNDLE"
  fi
fi

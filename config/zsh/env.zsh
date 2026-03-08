export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"

export EDITOR="nvim"

# Keep PATH unique while preserving order.
typeset -U path PATH

# Core system paths.
path=(/usr/sbin /sbin $path)

# Homebrew paths (Apple Silicon + Intel Rosetta).
if [[ "$OSTYPE" == darwin* ]]; then
  path=(/opt/homebrew/bin(N-/) /usr/local/bin(N-/) $path)
fi

### MANAGED BY RANCHER DESKTOP START (DO NOT EDIT)
path=("$HOME/.rd/bin" $path)
### MANAGED BY RANCHER DESKTOP END (DO NOT EDIT)

# Created by `pipx` on 2025-01-18 11:09:08
path=("$HOME/.local/bin" $path)

export PATH

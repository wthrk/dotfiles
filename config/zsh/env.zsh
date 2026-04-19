export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"

export EDITOR="nvim"

# Keep PATH unique while preserving order.
typeset -U path PATH

# Core system paths.
path=(/usr/sbin /sbin $path)

# Allowed unmanaged paths.
path=("$HOME/.agent-tools/bin" "$HOME/.rd/bin" $path)

# Keep user-local bins available but do not prioritize language-manager bins.
path=(
  "$HOME/.local/bin"
  $path
)

# Explicitly avoid old mutable language managers in priority path.
path=(${path:#$HOME/.nodebrew/current/bin})
path=(${path:#$HOME/.bun/bin})
path=(${path:#$HOME/.cargo/bin})
path=(${path:#$HOME/.pyenv/bin})
path=(${path:#$HOME/.rbenv/bin})

export PATH

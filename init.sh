#!/usr/bin/env bash
set -euo pipefail

mydir=$(cd "$(dirname "$0")" && pwd)

link_force() {
  local src="$1"
  local dst="$2"
  rm -rf "$dst"
  ln -s "$src" "$dst"
}

mkdir -p "$HOME/.config"

link_force "$mydir/.zshrc" "$HOME/.zshrc"
link_force "$mydir/config/zsh" "$HOME/.config/zsh"
link_force "$mydir/config/nvim" "$HOME/.config/nvim"
link_force "$mydir/.gitconfig" "$HOME/.gitconfig"

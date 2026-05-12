# Repo-managed zsh entrypoint.
# Home Manager may generate its own ~/.zshrc; this wrapper keeps direct sourcing usable.
if [ -f "$HOME/.config/zsh/.zshrc" ]; then
  source "$HOME/.config/zsh/.zshrc"
fi

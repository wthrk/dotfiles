# Main zsh entrypoint.
# Keep this file small and load feature-specific modules.

for file in \
  "$HOME/.config/zsh/env.zsh" \
  "$HOME/.config/zsh/options.zsh" \
  "$HOME/.config/zsh/history.zsh" \
  "$HOME/.config/zsh/aliases.zsh" \
  "$HOME/.config/zsh/completion.zsh" \
  "$HOME/.config/zsh/prompt.zsh" \
  "$HOME/.config/zsh/local.zsh"
do
  [ -f "$file" ] && source "$file"
done

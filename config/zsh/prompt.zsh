# powerlevel10k configuration
if [[ -o interactive ]] && [[ -t 0 ]] && [[ -t 1 ]] && [[ -r "$HOME/.config/zsh/p10k.zsh" ]]; then
  source "$HOME/.config/zsh/p10k.zsh"
fi

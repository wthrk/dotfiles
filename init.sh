#!/bin/bash
rm -f ~/.zshrc
ln -s ~/.dotfiles/.zshrc ~/.zshrc
mkdir -p ~/.config
rm -f ~/.config/nvim
ln -s ~/.dotfiles/config/nvim ~/.config/nvim

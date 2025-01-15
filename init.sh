#!/bin/bash
mydir=$(dirname $(readlink -f $0))
rm -f ~/.zshrc
ln -s $mydir/.zshrc ~/.zshrc
mkdir -p ~/.config
rm -f ~/.config/nvim
ln -s $mydir/nvim ~/.config/nvim
rm -f ~/.gitconfig
ln -s $mydir/.gitconfig ~/.gitconfig

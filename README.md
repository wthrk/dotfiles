# dotfiles

## zsh
- Run `./init.sh` to link `~/.zshrc` and `~/.config/zsh`.
- Main loader: `~/.zshrc` -> `~/.config/zsh/.zshrc`.
- Modular files:
  - `config/zsh/env.zsh`
  - `config/zsh/options.zsh`
  - `config/zsh/history.zsh`
  - `config/zsh/aliases.zsh`
  - `config/zsh/plugins.zsh`
  - `config/zsh/completion.zsh`
  - `config/zsh/prompt.zsh`
  - `config/zsh/local.zsh`
- Managed plugins (via antidote): `powerlevel10k`, `zsh-autosuggestions`,
  `fast-syntax-highlighting`, `fzf-tab`, `zsh-completions`.
- Recommended tools: `fzf`, `atuin`, `zoxide`.
- First prompt setup: run `p10k configure`.

## neovim
* install python3
  * on linux ```curl -L https://raw.githubusercontent.com/yyuu/pyenv-installer/master/bin/pyenv-installer | bash```
* install neovim python package. ```pip install neovim```
* open nvim

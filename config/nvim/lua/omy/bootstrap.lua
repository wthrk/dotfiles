-- VS Code Neovim では plugin manager を触らず、通常 Neovim だけ vim-jetpack を初期化する。
if not vim.g.vscode then
  omy.init_plugin_manager()
  require "omy.plugins"
end

-- どの言語や plugin でも共通に使う編集挙動を固定する。
vim.opt.expandtab = true
vim.opt.shiftwidth = 2
vim.opt.tabstop = 2
vim.opt.hidden = true
vim.opt.number = true
vim.opt.incsearch = true
vim.opt.showmatch = true
vim.opt.smartcase = true
vim.opt.smartindent = true
vim.opt.smarttab = true
vim.opt.whichwrap = "b,s,h,l,<,>,[,]"
vim.opt.scrolloff = 5
vim.opt.cursorline = true
vim.opt.ambiwidth = "single"
vim.opt.termguicolors = true
vim.opt.fileencodings = {
  "utf-8",
  "ucs-bom",
  "iso-2022-jp-3",
  "iso-2022-jp",
  "eucjp-ms",
  "euc-jisx0213",
  "euc-jp",
  "sjis",
  "cp932",
}

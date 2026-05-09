-- Treesitter parser と highlighting を有効化し、構文ベースの表示を安定させる。
require("nvim-treesitter.configs").setup {
  ensure_installed = "all",
  highlight = {
    enable = true,
  },
  indent = {
    enable = true,
  },
}

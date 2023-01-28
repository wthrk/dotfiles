vim.opt.completeopt = { "menu", "menuone", "noselect" }

local cmp = require "cmp"

cmp.setup {
  sources = cmp.config.sources {
    { name = "nvim_lsp" },
  },
}

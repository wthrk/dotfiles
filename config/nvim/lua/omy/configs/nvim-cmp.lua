vim.opt.completeopt = { "menu", "menuone", "noselect" }

local cmp = require "cmp"

cmp.setup {
  sources = cmp.config.sources({
    { name = "nvim_lsp" },
    { name = "nvim_lsp_signature_help" },
    { name = "path" },
    { name = "treesitter" },
  }, {
    { name = "buffer" },
  }),
}

cmp.setup.cmdline({ "/", "?" }, {
  mapping = cmp.mapping.preset.cmdline(),
  sources = {
    { name = "buffer" },
  },
})

cmp.setup.cmdline(":", {
  mapping = cmp.mapping.preset.cmdline(),
  sources = cmp.config.sources({
    { name = "path" },
  }, { { name = "cmdline" } }),
})

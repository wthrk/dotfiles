vim.opt.completeopt = { "menu", "menuone", "noselect" }

local cmp = require "cmp"

cmp.setup {
  sources = cmp.config.sources {
    { name = "nvim_lsp" },
  },
}

local lspconfig = require "lspconfig"
local lsp_defaults = lspconfig.util.default_config

lsp_defaults.capabilities = vim.tbl_deep_extend(
  "force",
  lsp_defaults.capabilities,
  require("cmp_nvim_lsp").default_capabilities()
)

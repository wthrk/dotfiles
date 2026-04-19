local mason_null_ls = require "mason-null-ls"

mason_null_ls.setup {
  ensure_installed = {},
  automatic_installation = false,
}

require "omy.configs.null-ls"

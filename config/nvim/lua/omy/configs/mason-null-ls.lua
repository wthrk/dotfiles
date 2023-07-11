local mason_null_ls = require "mason-null-ls"

mason_null_ls.setup {
  ensure_installed = { "stylua" },

}

require "omy.configs.null-ls"

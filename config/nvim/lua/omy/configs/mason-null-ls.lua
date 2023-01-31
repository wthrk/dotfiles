local mason_null_ls = require "mason-null-ls"

mason_null_ls.setup {
  ensure_installed = { "stylua" },
}

mason_null_ls.setup_handlers {
  function(source_name, methods)
    require "mason-null-ls.automatic_setup"(source_name, methods)
  end,
}

require "omy.configs.null-ls"

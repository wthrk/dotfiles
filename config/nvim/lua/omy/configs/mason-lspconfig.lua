local mason_lspconfig = require "mason-lspconfig"
local lspconfig = require "lspconfig"

mason_lspconfig.setup {
  ensure_installed = { "sumneko_lua", "rust_analyzer" },
}

mason_lspconfig.setup_handlers {
  function(server_name)
    lspconfig[server_name].setup {
      capabilities = require("cmp_nvim_lsp").default_capabilities(),
    }
  end,
}

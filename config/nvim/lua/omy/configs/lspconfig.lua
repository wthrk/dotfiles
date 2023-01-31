local lspconfig = require "lspconfig"

return {
  sumneko_lua = function(cababilities, on_attach)
    lspconfig.sumneko_lua.setup {
      settings = {
        Lua = {
          diagnostics = {
            globals = { "vim" },
          },
        },
      },
      cababilities = cababilities,
      on_attach = on_attach,
    }
  end,
}

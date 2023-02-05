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

  rust_analyzer = function(cababilities, on_attach)
    lspconfig.rust_analyzer.setup {
      settings = {
        ["rust-analyzer"] = {
          imports = {
            granularity = {
              group = "module",
            },
            prefix = "self",
          },
          cargo = {
            buildScripts = {
              enable = true,
            },
            features = "all",
          },
          procMacro = {
            enable = true,
          },
        },
      },
      cababilities = cababilities,
      on_attach = on_attach,
    }
  end,
}

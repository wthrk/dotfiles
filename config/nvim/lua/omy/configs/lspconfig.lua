local lspconfig = require "lspconfig"

return {
  lua_ls = function(cababilities, on_attach)
    lspconfig.lua_ls.setup {
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
            features = {},
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

  rescriptls = function(cababilities, on_attach)
    lspconfig.rescriptls.setup {
      settings = {
        codeLens = true,
        autoRunCodeAnalysis = true,
      },
      cababilities = cababilities,
      on_attach = on_attach,
    }
  end,
}

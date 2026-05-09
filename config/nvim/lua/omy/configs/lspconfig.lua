-- 各 language server の capabilities と個別設定を定義し、Mason/lspconfig から参照させる。
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

  ruby_lsp = function(cababilities, on_attach)
    lspconfig.ruby_lsp.setup {
      settings = {
        init_options = {
          formatter = true,
        },
      },
      cababilities = cababilities,
      on_attach = on_attach,
    }
  end,
}

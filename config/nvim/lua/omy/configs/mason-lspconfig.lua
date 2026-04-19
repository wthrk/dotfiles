local mason_lspconfig = require "mason-lspconfig"
local lspconfig = require "lspconfig"

mason_lspconfig.setup {
  ensure_installed = { "lua_ls", "rust_analyzer", "marksman", "ts_ls" },
}

local capabilities = require("cmp_nvim_lsp").default_capabilities()
local function on_attach(client, bufnr)
  require "omy.autocmds.lsp"(client, bufnr)
  require "omy.mappings.lsp"(client, bufnr)
end

local configs = {}
---@diagnostic disable-next-line: different-requires
for name, func in pairs(require "omy.configs.lspconfig") do
  configs[name] = function() func(capabilities, on_attach) end
end

mason_lspconfig.setup_handlers(vim.tbl_extend("force", {
  function(server_name)
    lspconfig[server_name].setup {
      capabilities = capabilities,
      on_attach = on_attach,
    }
  end,
}, configs))

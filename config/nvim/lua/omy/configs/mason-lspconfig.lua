local mason_lspconfig = require "mason-lspconfig"
local lspconfig = require "lspconfig"

mason_lspconfig.setup {
  ensure_installed = { "sumneko_lua", "rust_analyzer" },
}

local capabilities = require("cmp_nvim_lsp").default_capabilities()
local function on_attach(client, bufnr)
  -- auto format の切り替え機能を入れる？
  vim.api.nvim_buf_create_user_command(
    bufnr,
    "Format",
    function() vim.lsp.buf.format { bufnr = bufnr } end,
    { desc = "Format file with LSP" }
  )
  local autocmd_group = "auto_format_" .. bufnr
  vim.api.nvim_create_augroup(autocmd_group, { clear = true })
  vim.api.nvim_create_autocmd("BufWritePre", {
    group = autocmd_group,
    buffer = bufnr,
    desc = "Auto format buffer " .. bufnr .. " before save",
    callback = function() vim.lsp.buf.format { bufnr = bufnr } end,
  })
  -- lsp mapping をここに持ってくる
end

mason_lspconfig.setup_handlers {
  function(server_name)
    lspconfig[server_name].setup {
      capabilities = capabilities,
      on_attach = on_attach,
    }
  end,
}

require "omy.configs.lspconfig"

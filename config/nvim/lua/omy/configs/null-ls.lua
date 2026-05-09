-- 外部 formatter を null-ls 経由に限定し、LSP server 本体の formatter と競合させない。
local null_ls = require "null-ls"

local augroup = vim.api.nvim_create_augroup("LspFormatting", {})

null_ls.setup {
  sources = {
    null_ls.builtins.formatting.stylua,
    null_ls.builtins.formatting.markdownlint,
  },

  debug = false,

  on_attach = function(client, bufnr)
    -- 保存時 formatting は null-ls が attach した buffer だけで有効にする。
    -- https://github.com/jose-elias-alvarez/null-ls.nvim/wiki/Formatting-on-save
    if client.supports_method "textDocument/formatting" then
      vim.api.nvim_clear_autocmds { group = augroup, buffer = bufnr }
      vim.api.nvim_create_autocmd("BufWritePre", {
        group = augroup,
        buffer = bufnr,
        callback = function()
          vim.lsp.buf.format {
            filter = function(client_)
              -- 複数 client が formatting を提供しても、保存時は null-ls だけに絞る。
              return client_.name == "null-ls"
            end,
            bufnr = bufnr,
          }
        end,
      })
    end
  end,
}

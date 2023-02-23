local null_ls = require "null-ls"

local augroup = vim.api.nvim_create_augroup("LspFormatting", {})

null_ls.setup {
  sources = {
    null_ls.builtins.diagnostics.eslint,
    null_ls.builtins.formatting.stylua,
    null_ls.builtins.formatting.markdownlint,
    --null_ls.builtins.formatting.prettier_eslint,
  },

  debug = false,

  on_attach = function(client, bufnr)
    -- fix on save.
    -- https://github.com/jose-elias-alvarez/null-ls.nvim/wiki/Formatting-on-save
    if client.supports_method "textDocument/formatting" then
      vim.api.nvim_clear_autocmds { group = augroup, buffer = bufnr }
      vim.api.nvim_create_autocmd("BufWritePre", {
        group = augroup,
        buffer = bufnr,
        callback = function()
          vim.lsp.buf.format {
            filter = function(client_)
              -- apply whatever logic you want (in this example, we'll only use null-ls)
              return client_.name == "null-ls"
            end,
            bufnr = bufnr,
          }
        end,
      })
    end
  end,
}

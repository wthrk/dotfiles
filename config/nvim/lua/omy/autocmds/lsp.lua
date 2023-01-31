return function(client, bufnr)
  local capabilities = client.server_capabilities

  if capabilities.documentFormattingProvider then
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
  end
end

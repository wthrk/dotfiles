return function(client, bufnr)
  local capabilities = client.server_capabilities
  local function noremapbuf(mode, lhs, rhs, opt)
    omy.noremap(
      mode,
      lhs,
      rhs,
      vim.tbl_extend("force", opt or {}, { buffer = bufnr })
    )
  end

  noremapbuf(
    "n",
    "<leader>ld",
    "<cmd>Lspsaga show_line_diagnostics<cr>",
    { desc = "Hover diagnostics", silent = true }
  )
  noremapbuf(
    "n",
    "gl",
    "<cmd>Lspsaga show_line_diagnostics<cr>",
    { desc = "Hover diagnostics", silent = true }
  )
  noremapbuf(
    "n",
    "[d",
    "<cmd>Lspsaga diagnostic_jump_prev<cr>",
    { desc = "Previous diagnostic", silent = true }
  )
  noremapbuf(
    "n",
    "]d",
    "<cmd>Lspsaga diagnostic_jump_next<cr>",
    { desc = "Next diagnostic", silent = true }
  )

  if capabilities.codeActionProvider then
    noremapbuf(
      { "n", "v" },
      "<leader>la",
      "<cmd>Lspsaga code_action<cr>",
      { desc = "LSP code action", silent = true }
    )
  end

  if capabilities.codeLensProvider then
    noremapbuf(
      "n",
      "<leader>ll",
      function() vim.lsp.codelens.refresh() end,
      { desc = "LSP codelens refresh" }
    )
    noremapbuf(
      "n",
      "<leader>lL",
      function() vim.lsp.codelens.run() end,
      { desc = "LSP codelens run" }
    )
  end

  if capabilities.declarationProvider then
    noremapbuf(
      "n",
      "gD",
      function() vim.lsp.buf.declaration() end,
      { desc = "Declaration of current symbol" }
    )
  end

  if capabilities.definitionProvider then
    noremapbuf(
      "n",
      "gd",
      function() vim.lsp.buf.definition() end,
      { desc = "Show the definition of current symbol" }
    )
  end

  if capabilities.documentFormattingProvider then
    noremapbuf("n", "<leader>lf", ":Format", { desc = "Format buffer" })
  end

  if capabilities.hoverProvider then
    noremapbuf(
      "n",
      "K",
      "<cmd>Lspsaga hover_doc<cr>",
      { desc = "Hover symbol details", silent = true }
    )
  end

  if capabilities.implementationProvider then
    noremapbuf(
      "n",
      "gI",
      function() vim.lsp.buf.implementation() end,
      { desc = "Implementation of current symbol" }
    )
  end

  if capabilities.referencesProvider then
    noremapbuf(
      "n",
      "gr",
      function() vim.lsp.buf.references() end,
      { desc = "References of current symbol" }
    )
    noremapbuf(
      "n",
      "<leader>lR",
      function() vim.lsp.buf.references() end,
      { desc = "Search references" }
    )
  end

  if capabilities.renameProvider then
    noremapbuf(
      "n",
      "<leader>lr",
      "<cmd>Lspsaga rename<cr>",
      { desc = "Rename current symbol", silent = true }
    )
  end

  if capabilities.signatureHelpProvider then
    noremapbuf(
      "n",
      "<leader>lh",
      function() vim.lsp.buf.signature_help() end,
      { desc = "Signature help" }
    )
  end

  if capabilities.typeDefinitionProvider then
    noremapbuf(
      "n",
      "gT",
      function() vim.lsp.buf.type_definition() end,
      { desc = "Definition of current type" }
    )
  end

  if capabilities.workspaceSymbolProvider then
    noremapbuf(
      "n",
      "<leader>lG",
      function() vim.lsp.buf.workspace_symbol() end,
      { desc = "Search workspace symbols" }
    )
  end
end

return function(client, bufnr)
  local capabilities = client.server_capabilities
  local function noremap(mode, lhs, rhs, opt)
    vim.keymap.set(
      mode,
      lhs,
      rhs,
      vim.tbl_extend("force", opt or {}, { noremap = true, buffer = bufnr })
    )
  end

  noremap(
    "n",
    "<leader>ld",
    function() vim.diagnostic.open_float() end,
    { desc = "Hover diagnostics" }
  )
  noremap(
    "n",
    "[d",
    function() vim.diagnostic.goto_prev() end,
    { desc = "Previous diagnostic" }
  )
  noremap(
    "n",
    "]d",
    function() vim.diagnostic.goto_next() end,
    { desc = "Next diagnostic" }
  )
  noremap(
    "n",
    "gl",
    function() vim.diagnostic.open_float() end,
    { desc = "Hover diagnostics" }
  )

  if capabilities.codeActionProvider then
    noremap(
      "n",
      "<leader>la",
      function() vim.lsp.buf.code_action() end,
      { desc = "LSP code action" }
    )
    noremap(
      "v",
      "<leader>la",
      function() vim.lsp.buf.code_action() end,
      { desc = "LSP code action" }
    )
  end

  if capabilities.codeLensProvider then
    noremap(
      "n",
      "<leader>ll",
      function() vim.lsp.codelens.refresh() end,
      { desc = "LSP codelens refresh" }
    )
    noremap(
      "n",
      "<leader>lL",
      function() vim.lsp.codelens.run() end,
      { desc = "LSP codelens run" }
    )
  end

  if capabilities.declarationProvider then
    noremap(
      "n",
      "gD",
      function() vim.lsp.buf.declaration() end,
      { desc = "Declaration of current symbol" }
    )
  end

  if capabilities.definitionProvider then
    noremap(
      "n",
      "gd",
      function() vim.lsp.buf.definition() end,
      { desc = "Show the definition of current symbol" }
    )
  end

  if capabilities.documentFormattingProvider then
    noremap("n", "<leader>lf", ":Format", { desc = "Format buffer" })
  end

  if capabilities.hoverProvider then
    noremap(
      "n",
      "K",
      function() vim.lsp.buf.hover() end,
      { desc = "Hover symbol details" }
    )
  end

  if capabilities.implementationProvider then
    noremap(
      "n",
      "gI",
      function() vim.lsp.buf.implementation() end,
      { desc = "Implementation of current symbol" }
    )
  end

  if capabilities.referencesProvider then
    noremap(
      "n",
      "gr",
      function() vim.lsp.buf.references() end,
      { desc = "References of current symbol" }
    )
    noremap(
      "n",
      "<leader>lR",
      function() vim.lsp.buf.references() end,
      { desc = "Search references" }
    )
  end

  if capabilities.renameProvider then
    noremap(
      "n",
      "<leader>lr",
      function() vim.lsp.buf.rename() end,
      { desc = "Rename current symbol" }
    )
  end

  if capabilities.signatureHelpProvider then
    noremap(
      "n",
      "<leader>lh",
      function() vim.lsp.buf.signature_help() end,
      { desc = "Signature help" }
    )
  end

  if capabilities.typeDefinitionProvider then
    noremap(
      "n",
      "gT",
      function() vim.lsp.buf.type_definition() end,
      { desc = "Definition of current type" }
    )
  end

  if capabilities.workspaceSymbolProvider then
    noremap(
      "n",
      "<leader>lG",
      function() vim.lsp.buf.workspace_symbol() end,
      { desc = "Search workspace symbols" }
    )
  end
end

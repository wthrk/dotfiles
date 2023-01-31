local function noremap(mode, lhs, rhs, opt)
  vim.keymap.set(
    mode,
    lhs,
    rhs,
    vim.tbl_extend("force", opt or {}, { noremap = true })
  )
end

-- Leader key
vim.g.mapleader = " "

-- ;: 切り替え
noremap("n", ";", ":")
noremap("n", ":", ";")

-- for LSP
-- どうしようか考える
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
noremap(
  "n",
  "gD",
  function() vim.lsp.buf.declaration() end,
  { desc = "Declaration of current symbol" }
)
noremap(
  "n",
  "gd",
  function() vim.lsp.buf.definition() end,
  { desc = "Show the definition of current symbol" }
)
noremap(
  "n",
  "K",
  function() vim.lsp.buf.hover() end,
  { desc = "Hover symbol details" }
)
noremap(
  "n",
  "gI",
  function() vim.lsp.buf.implementation() end,
  { desc = "Implementation of current symbol" }
)
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
noremap(
  "n",
  "<leader>lr",
  function() vim.lsp.buf.rename() end,
  { desc = "Rename current symbol" }
)
noremap(
  "n",
  "<leader>lh",
  function() vim.lsp.buf.signature_help() end,
  { desc = "Signature help" }
)
noremap(
  "n",
  "gT",
  function() vim.lsp.buf.type_definition() end,
  { desc = "Definition of current type" }
)
noremap(
  "n",
  "<leader>lG",
  function() vim.lsp.buf.workspace_symbol() end,
  { desc = "Search workspace symbols" }
)

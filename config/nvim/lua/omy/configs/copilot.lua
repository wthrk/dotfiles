vim.g.copilot_assume_mapped = true
vim.api.nvim_set_keymap(
  "i",
  "<C-F>",
  'copilot#Accept("<CR>")',
  { silent = true, expr = true }
)
vim.g.copilot_filetypes = {
  ["*"] = true,
  -- ["javascript"] = true,
  -- ["typescript"] = true,
  -- ["javascriptreact"] = true,
  -- ["typescriptreact"] = true,
  -- ["rust"] = true,
  -- ["c"] = true,
  -- ["c#"] = true,
  -- ["c++"] = true,
  -- ["go"] = true,
  -- ["python"] = true,
  -- ["rescript"] = true,
  -- ["ruby"] = true,
}

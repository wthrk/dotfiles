-- `vim.ui.input/select` を dressing に差し替え、LSP rename などの入力 UI を揃える。
require("dressing").setup {
  input = {
    default_prompt = "➤ ",
    win_options = { winhighlight = "Normal:Normal,NormalNC:Normal" },
  },
  select = {
    backend = { "telescope", "builtin" },
    builtin = {
      win_options = { winhighlight = "Normal:Normal,NormalNC:Normal" },
    },
  },
}

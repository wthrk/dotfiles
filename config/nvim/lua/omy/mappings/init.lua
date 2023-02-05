-- Leader key
vim.g.mapleader = " "

local noremap = omy.noremap

-- ;: 切り替え
noremap("n", ";", ":")
noremap("n", ":", ";")

-- neo-tree
noremap(
  "n",
  "<leader>e",
  "<cmd>Neotree toggle<cr>",
  { desc = "Toggle Explorer" }
)
noremap("n", "<leader>o", "<cmd>Neotree focus<cr>", { desc = "Focus Explorer" })

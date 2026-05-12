-- ファイル操作、検索、表示切り替えなど、LSP に依存しない keymap を定義する。
--
-- Leader key
vim.g.mapleader = " "

local noremap = omy.noremap

-- セミコロンでコマンドラインへ入り、通常モードの移動と競合しないようにする。
noremap("n", ";", ":")
noremap("n", ":", ";")

-- neo-tree はファイルツリー表示と Git 状態確認に使う。
noremap(
  "n",
  "<leader>e",
  "<cmd>Neotree toggle<cr>",
  { desc = "Toggle Explorer" }
)
noremap("n", "<leader>o", "<cmd>Neotree focus<cr>", { desc = "Focus Explorer" })

-- markdown-preview は Markdown buffer でだけ使い、ブラウザで表示確認する。
noremap(
  "n",
  "<Leader>mp",
  ":MarkdownPreview<CR>",
  { desc = "Preview Mardown on the brower" }
)

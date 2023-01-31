-- telescope
local noremap = omy.noremap
local builtin = require "telescope.builtin"
---@diagnostic disable-next-line: different-requires
local extensions = require("telescope").extensions

noremap("n", "<leader>ff", builtin.find_files)
noremap("n", "<leader>fg", builtin.live_grep)
noremap("n", "<leader>fb", builtin.buffers)
noremap("n", "<leader>fh", builtin.help_tags)
noremap(
  "n",
  "<leader><leader>",
  function() extensions.frecency.frecency { workspace = "CWD" } end,
  { silent = true }
)

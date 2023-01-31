-- telescope
local noremap = omy.noremap
local builtin = require "telescope.builtin"

noremap("n", "<leader>ff", builtin.find_files)
noremap("n", "<leader>fg", builtin.live_grep)
noremap("n", "<leader>fb", builtin.buffers)
noremap("n", "<leader>fh", builtin.help_tags)

vim.cmd "packadd vim-jetpack"
require("jetpack.packer").startup(function(use)
  use { "tani/vim-jetpack", opt = 1 }
  use { "lewis6991/impatient.nvim", ["do"] = function() require "impatient" end }
  use "rbtnn/vim-ambiwidth"
end)

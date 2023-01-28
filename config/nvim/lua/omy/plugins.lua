vim.cmd "packadd vim-jetpack"
require("jetpack.packer").add {
  { "tani/vim-jetpack", opt = 1 },
  { "lewis6991/impatient.nvim", run = function() require "impatient" end },
  "rbtnn/vim-ambiwidth",
  {
    "nvim-treesitter/nvim-treesitter",
    run = ":TSUpdate",
    config = function() require "omy.configs.nvim-treesitter" end,
  },
  {
    "williamboman/mason.nvim",
    as = "mason",
  },
  {
    "neovim/nvim-lspconfig",
    as = "lspconfig",
  },
  {
    "williamboman/mason-lspconfig.nvim",
    after = { "mason", "lspconfig" },
    config = function()
      require "omy.configs.mason"
      require "omy.configs.mason-lspconfig"
    end,
  },
  "hrsh7th/cmp-nvim-lsp",
  {
    "hrsh7th/nvim-cmp",
    config = function() require "omy.configs.nvim-cmp" end,
  },
}

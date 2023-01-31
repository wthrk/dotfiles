vim.cmd "packadd vim-jetpack"
require("jetpack.packer").add {
  { "tani/vim-jetpack", opt = 1 },
  {
    "nvim-lua/plenary.nvim",
    as = "plenary",
  },
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
    config = function() require "omy.configs.mason" end,
  },
  {
    "neovim/nvim-lspconfig",
    as = "lspconfig",
  },
  {
    "williamboman/mason-lspconfig.nvim",
    after = { "mason", "lspconfig", "cmp" },
    config = function() require "omy.configs.mason-lspconfig" end,
  },
  "hrsh7th/cmp-nvim-lsp",
  "hrsh7th/cmp-nvim-lsp-signature-help",
  "hrsh7th/cmp-buffer",
  "hrsh7th/cmp-path",
  "hrsh7th/cmp-cmdline",
  "hrsh7th/cmp-nvim-lua",
  "ray-x/cmp-treesitter",
  {
    "hrsh7th/nvim-cmp",
    config = function() require "omy.configs.nvim-cmp" end,
    as = "cmp",
  },
  {
    "jose-elias-alvarez/null-ls.nvim",
    as = "null-ls",
    after = { "plenary" },
  },
  {
    "jay-babu/mason-null-ls.nvim",
    after = { "null-ls", "mason" },
    config = function() require "omy.configs.mason-null-ls" end,
  },
}

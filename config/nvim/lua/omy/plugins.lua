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
    "kyazdani42/nvim-web-devicons",
    as = "web-devicons",
    config = function() require "omy.configs.web-devicons" end,
  },
  {
    "kkharji/sqlite.lua",
    as = "sqlite",
  },
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
  {
    "kkharji/lspsaga.nvim",
    config = function() require "omy.configs.lspsaga" end,
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
    requires = { "plenary" },
  },
  {
    "jay-babu/mason-null-ls.nvim",
    after = { "null-ls", "mason" },
    config = function() require "omy.configs.mason-null-ls" end,
  },
  {
    "nvim-telescope/telescope.nvim",
    as = "telescope",
    requires = { "plenary" },
    ---@diagnostic disable-next-line: different-requires
    config = function() require "omy.configs.telescope" end,
  },
  {
    "nvim-telescope/telescope-frecency.nvim",
    requires = { "sqlite", "web-devicons" },
  },
  {
    "stevearc/dressing.nvim",
    config = function() require "omy.configs.dressing" end,
  },
  {
    "EdenEast/nightfox.nvim",
    config = function() require "omy.configs.nightfox" end,
  },
  {
    "j-hui/fidget.nvim",
    config = function() require "omy.configs.figet" end,
  },
  {
    "nvim-lualine/lualine.nvim",
    requires = { "web-devicons" },
    config = function() require "omy.configs.lualine" end,
  },
}

-- vim-jetpack に渡す plugin 一覧。設定本体は `configs/*` へ分け、読み込み順だけここで決める。
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
    "MunifTanjim/nui.nvim",
    as = "nui",
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
  {
    "onsails/lspkind.nvim",
    config = function() require "omy.configs.lspkind" end,
  },
  "hrsh7th/cmp-nvim-lsp",
  "hrsh7th/cmp-nvim-lsp-signature-help",
  "hrsh7th/cmp-buffer",
  "hrsh7th/cmp-path",
  "hrsh7th/cmp-cmdline",
  "hrsh7th/cmp-nvim-lua",
  "hrsh7th/cmp-vsnip",
  "hrsh7th/vim-vsnip",
  "hrsh7th/vim-vsnip-integ",
  "ray-x/cmp-treesitter",
  {
    "hrsh7th/nvim-cmp",
    config = function() require "omy.configs.nvim-cmp" end,
    as = "cmp",
  },
  {
    "nvimtools/none-ls.nvim",
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
  {
    "nvim-neo-tree/neo-tree.nvim",
    requires = { "web-devicons", "plenary", "nui" },
    config = function() require "omy.configs.neo-tree" end,
  },
  {
    "iamcco/markdown-preview.nvim",
    run = function() vim.fn["mkdp#util#install"]() end,
    setup = function() vim.g.mkdp_filetypes = { "markdown" } end,
    ft = { "markdown" },
  },
  "dhruvasagar/vim-table-mode",
  {
    "folke/which-key.nvim",
    config = function() require "omy.configs.which-key" end,
  },
  "vim-firestore",
  "ftdetect/firestore.vim",
  {
    "github/copilot.vim",
    setup = function() require "omy.configs.copilot" end,
  },
  "nkrkv/nvim-treesitter-rescript",
}

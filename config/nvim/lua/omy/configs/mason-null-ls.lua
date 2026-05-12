-- Mason が取得した formatter / diagnostic tool を null-ls の source として使えるようにする。
local mason_null_ls = require "mason-null-ls"

mason_null_ls.setup {
  ensure_installed = {},
  automatic_installation = false,
}

require "omy.configs.null-ls"

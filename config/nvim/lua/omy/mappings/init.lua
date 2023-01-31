local function noremap(mode, lhs, rhs, opt)
  vim.keymap.set(
    mode,
    lhs,
    rhs,
    vim.tbl_extend("force", opt or {}, { noremap = true })
  )
end

-- Leader key
vim.g.mapleader = " "

-- ;: 切り替え
noremap("n", ";", ":")
noremap("n", ":", ";")

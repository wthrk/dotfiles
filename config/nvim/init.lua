-- Home Manager がリンクした Lua 設定を、失敗箇所が分かる形で順に読み込む。
--
-- plugin manager の初期化、keymap、基本 option を分け、どの require で壊れたか表示する。
local impatient_ok, impatient = pcall(require, "impatient")
if impatient_ok then impatient.enable_profile() end

local init_sources = {
  "omy.util",
  "omy.bootstrap",
  "omy.mappings",
  "omy.opt",
}

for _, source in ipairs(init_sources) do
  local status_ok, fault = pcall(require, source)
  if not status_ok then
    vim.api.nvim_err_writeln("Failed to load " .. source .. "\n\n" .. fault)
  end
end

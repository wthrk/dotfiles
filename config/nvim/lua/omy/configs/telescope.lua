-- Telescope picker の表示と ignore ルールを揃え、検索結果が不要ファイルで埋まらないようにする。
require("telescope").setup {}
require("telescope").load_extension "frecency"
---@diagnostic disable-next-line: different-requires
require "omy.mappings.telescope"

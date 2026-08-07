# Home Manager の `programs.neovim` を有効化する。
#
# Node/Python/Ruby provider を有効にし、`vi`/`vim` の alias も Neovim に寄せる。
#
# provider を有効にすると Home Manager は host prog（`vim.g.node_host_prog` 等）を書いた
# `xdg.configFile."nvim/init.lua"` を生成する。`config/nvim` をディレクトリごとリンクすると
# この entry と衝突し、Home Manager は生成側を破棄する（build ログの
# `.config/nvim/init.lua conflicts with recursively symlinked file`）。結果 provider は
# closure に入るのに一度も配線されず、`withNodeJs` などが死んだ依存になる。
#
# そのため init.lua は Home Manager に一本化し、リポジトリの Lua 本体は `initLua` として
# provider 設定の後ろへ連結する。`lua/` 以下は `shell-files.nix` が別途リンクし、`require("omy.*")`
# の解決経路を保つ。
{ root, ... }:
{
  programs.neovim = {
    enable = true;
    defaultEditor = true;
    viAlias = true;
    vimAlias = true;
    withNodeJs = true;
    withPython3 = true;
    withRuby = true;
    initLua = builtins.readFile "${root}/config/nvim/init.lua";
  };
}

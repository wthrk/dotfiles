# Home Manager の `programs.neovim` を有効化する。
#
# Node/Python/Ruby provider を有効にし、`vi`/`vim` の alias も Neovim に寄せる。Lua 設定本体は
# `shell-files.nix` が `config/nvim` をリンクする。
{
  programs.neovim = {
    enable = true;
    defaultEditor = true;
    viAlias = true;
    vimAlias = true;
    withNodeJs = true;
    withPython3 = true;
    withRuby = true;
  };
}

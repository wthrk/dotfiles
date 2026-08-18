# リポジトリ内の設定ディレクトリを Home Manager のリンク対象にする。
#
# 引数 `root` は参照元リポジトリのパスで、生成されたローカル flake の場所には依存しない。
# zsh と Neovim 設定は `force = true` でリンクを更新し、旧配置が残っても管理対象へ戻す。
#
# Neovim は `config/nvim` をディレクトリごとリンクしない。それをすると `programs.neovim` が
# provider host prog のために生成する `nvim/init.lua` と衝突し、生成側が破棄されて provider が
# 配線されなくなる（`neovim.nix` 参照）。init.lua は `neovim.nix` の `extraLuaConfig` が持ち、
# ここは `require("omy.*")` の解決に要る `lua/` 以下だけをリンクする。
{
  lib,
  pkgs,
  root,
  ...
}:
{
  xdg.enable = true;

  xdg.configFile."zsh" = {
    source = "${root}/config/zsh";
    force = true;
  };

  xdg.configFile."nvim/lua" = {
    source = "${root}/config/nvim/lua";
    force = true;
  };

  xdg.configFile."nvim/.stylua.toml" = {
    source = "${root}/config/nvim/.stylua.toml";
    force = true;
  };

  # Karabiner-Elements のキー割り当て。Karabiner 本体は Homebrew cask で入れる（`nix/modules/homebrew.nix`）ので、
  # `karabiner.json` は宣言側のこのリンクが正本になる。読み込み先は `~/.config/karabiner/karabiner.json` 固定。
  # Karabiner は macOS 専用なので `mkHome` が Linux で評価されたときはリンクしない。
  #
  # Karabiner が設定を保存すると store への symlink が実ファイルに置き換わる。`force = true` は、その実ファイルを
  # 次の activation で宣言側へ戻すために要る。保存の機構と、何を宣言に入れるかは `README.md` のキーリマップ節が持つ。
  xdg.configFile."karabiner/karabiner.json" = lib.mkIf pkgs.stdenv.isDarwin {
    source = "${root}/config/karabiner/karabiner.json";
    force = true;
  };

  # 入力ソースを ABC へ戻す Hammerspoon 設定。Hammerspoon 本体は Homebrew cask で入れる
  # （`nix/modules/homebrew.nix`）ので、`init.lua` は宣言側のこのリンクが正本になる。読み込み先は
  # `~/.hammerspoon/init.lua` 固定で XDG 配下ではないため、`xdg.configFile` ではなく `home.file` に置く。
  # Hammerspoon は macOS 専用なので `mkHome` が Linux で評価されたときはリンクしない。
  home.file.".hammerspoon/init.lua" = lib.mkIf pkgs.stdenv.isDarwin {
    source = "${root}/config/hammerspoon/init.lua";
    force = true;
  };
}

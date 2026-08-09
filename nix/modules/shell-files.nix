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

  # Karabiner-Elements のキー割り当て。`services.karabiner-elements`（`nix/darwin.nix`）は daemon と driver
  # だけを配線し、`karabiner.json` は管理しないのでここでリンクする。Karabiner は macOS 専用なので
  # `mkHome` が Linux で評価されたときはリンクしない。
  #
  # Karabiner は設定を保存するとき対象を unlink してから書き直すため、GUI で設定を変えると store への
  # symlink が実ファイルに置き換わる。宣言側を正本とし GUI から触らない運用にする。`force = true` は
  # そのとき置き換わった実ファイルを次の activation で宣言側へ戻すために要る。
  xdg.configFile."karabiner/karabiner.json" = lib.mkIf pkgs.stdenv.isDarwin {
    source = "${root}/config/karabiner/karabiner.json";
    force = true;
  };
}

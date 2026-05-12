# リポジトリ内の設定ディレクトリを Home Manager のリンク対象にする。
#
# 引数 `root` は参照元リポジトリのパスで、生成されたローカル flake の場所には依存しない。
# zsh と Neovim 設定は `force = true` でリンクを更新し、旧配置が残っても管理対象へ戻す。
{ root, ... }:
{
  xdg.enable = true;

  xdg.configFile."zsh" = {
    source = "${root}/config/zsh";
    force = true;
  };

  xdg.configFile."nvim" = {
    source = "${root}/config/nvim";
    force = true;
  };
}

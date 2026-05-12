# Home Manager 管理のシェルで direnv と nix-direnv を有効化する。
#
# 補完、キーバインド、プロンプト表示は zsh 側の設定に寄せ、このモジュールでは
# `programs.direnv` の有効化だけを担う。
{
  programs.direnv = {
    enable = true;
    nix-direnv.enable = true;
  };
}

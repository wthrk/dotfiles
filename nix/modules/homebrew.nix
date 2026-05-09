# nix-darwin 経由で Homebrew の tap、formula、cask を宣言する。
#
# tap の実体は `nix/darwin.nix` の nix-homebrew 設定で flake input に固定する。このモジュールでは
# brew bundle 相当の内容だけを宣言し、手動 tap 更新に依存しない。
{
  homebrew = {
    enable = true;
    taps = [
      "homebrew/homebrew-core"
      "homebrew/homebrew-cask"
      {
        name = "azure/bicep";
        clone_target = "https://github.com/Azure/homebrew-bicep";
      }
      {
        name = "hashicorp/tap";
        clone_target = "https://github.com/hashicorp/homebrew-tap";
      }
    ];

    onActivation = {
      autoUpdate = false;
      upgrade = false;
      cleanup = "uninstall";
    };

    casks = [
      "font-cica"
    ];

    masApps = { };
  };
}

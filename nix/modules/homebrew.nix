# nix-darwin の `homebrew` option に渡す Homebrew 宣言。
#
# `taps` は brew bundle が参照する tap 名、`casks` は switch 時に導入する cask、
# `onActivation` は switch 時の更新と cleanup の扱いを指定する。
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

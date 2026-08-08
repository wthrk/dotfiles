# nix-darwin の `homebrew` option に渡す Homebrew 宣言。
{ homebrewTaps, ... }:
{
  homebrew = {
    enable = true;
    taps = map (tap: tap.brewTap) homebrewTaps;

    onActivation = {
      # tap は flake input rev で固定するため、switch 時の暗黙 tap 取得は行わない。
      autoUpdate = false;
      # auto-update daemon の `dotfiles update` から実機の installed を tap pin へ収束させる。
      upgrade = true;
      # 宣言外パッケージをアンインストールする。nix-darwin はこの値のとき brew bundle へ --force-cleanup を
      # 渡すため、lock 済み brew がそのフラグを持つことが前提になる（下限は
      # `cargo xtask check static` の BREW_REF_WITH_FORCE_CLEANUP）。
      cleanup = "uninstall";
    };

    # `auto_updates true` / `version :latest` の cask も含め、全 cask を tap pin 追従の無人 upgrade 対象に
    # する。前提となる「全 cask が sha256 固定」の受容根拠と確認手順は
    # docs/automation/homebrew-cask-pinning.md を正本とする。
    greedyCasks = true;

    # `kicad` は nixpkgs 側が `broken = stdenv.hostPlatform.isDarwin` のため macOS では評価に通らず、cask で
    # 宣言する。nixpkgs と cask のどちらで宣言するかの規約は docs/automation/homebrew-cask-pinning.md を
    # 正本とする。
    casks = [
      "azookey"
      "bitwarden"
      "codex-app"
      "font-cica"
      "ghostty"
      "kicad"
      "yubico-authenticator"
    ];

    masApps = { };
  };
}

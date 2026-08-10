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
      # 宣言外パッケージをアンインストールする。nix-darwin はこの値のとき brew bundle へ --force-cleanup を渡す。
      cleanup = "uninstall";
    };

    # `auto_updates true` / `version :latest` の cask も含め、全 cask を tap pin 追従の無人 upgrade 対象に
    # する。前提となる「全 cask が sha256 固定」の受容根拠と強制機構は
    # docs/automation/homebrew-cask-pinning.md を正本とする。
    greedyCasks = true;

    # `kicad` は nixpkgs 側が `broken = stdenv.hostPlatform.isDarwin` のため macOS では評価に通らず、cask で
    # 宣言する。
    #
    # `claude-code@latest` は `nix/modules/cli.nix` の bunx wrapper に載せられないため cask で宣言する。
    # npm 版の中身は platform 別 optional dependency に入った native binary を postinstall がリンクする方式で、
    # bun は lifecycle script を既定で実行しないためリンクが走らない。加えて Claude Code は自前でバックグラウンド
    # 自動更新するので、bunx のキャッシュ管理と衝突する。cask 名の `@latest` は latest チャンネルを指す
    # （`claude-code` は stable チャンネル）。
    casks = [
      "azookey"
      "bitwarden"
      "claude-code@latest"
      "codex-app"
      "font-cica"
      "ghostty"
      "kicad"
      "yubico-authenticator"
    ];

    masApps = { };
  };
}

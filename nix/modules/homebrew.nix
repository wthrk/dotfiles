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
    # `claude-code@latest` は cask で宣言する。Claude Code は導入経路を見て自動更新の可否を決めるため、cask 版だけが
    # 自動更新を止めて `onActivation.upgrade` と `greedyCasks` の無人 upgrade に乗る。`nix/modules/cli.nix` の bunx
    # wrapper に載せた版は自動更新を試みて更新先が書けずに失敗し、版が固定される。
    # cask 名の `@latest` は latest チャンネルを指す（`claude-code` は stable チャンネル）。
    #
    # `karabiner-elements` は CapsLock→Ctrl の remap 実体で、キー割り当ては
    # `config/karabiner/karabiner.json`（`nix/modules/shell-files.nix` がリンク）が持つ。
    # `services.karabiner-elements.enable` は使わない。pin 済み `karabiner-elements-15.7.0` には上流モジュールが
    # `environment.userLaunchAgents` へ載せる LaunchAgents plist が無く、`userLaunchd` activation の `cp -f` が
    # dangling symlink を掴んで止まるためである。そのため `launchd.user.agents` は空に保つ。
    casks = [
      "azookey"
      "bitwarden"
      "claude-code@latest"
      "codex-app"
      "font-cica"
      "ghostty"
      "karabiner-elements"
      "kicad"
      "yubico-authenticator"
    ];

    masApps = { };
  };
}

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
    # dangling symlink を掴んで止まるためである。
    #
    # `hammerspoon` は tty アプリと Zed のフォーカス時に入力ソースを ABC へ戻す実体で、対象アプリと
    # 強制処理は `config/hammerspoon/init.lua`（`nix/modules/shell-files.nix` がリンク）が持ち、起動は
    # `nix/modules/hammerspoon.nix` の LaunchAgent が担う。
    # Karabiner の `select_input_source` は `from` のキーイベントを要求するため、アプリのアクティブ化を
    # トリガーにできない。
    casks = [
      "antigravity"
      "antigravity-cli"
      "azookey"
      "bitwarden"
      "claude"
      "claude-code@latest"
      "codex-app"
      "discord"
      "firefox"
      "font-cica"
      "ghostty"
      # upstream cask が `sha256 :no_check` となり、greedyCasks の「全成果物を固定する」
      # 前提を満たさない。Chrome は `cli.nix` が mmdc 用に Nix package として条件付きで提供するため、
      # Homebrew の無人更新対象には含めない。
      "hammerspoon"
      "iterm2"
      "karabiner-elements"
      "kicad"
      "notion"
      "slack"
      "visual-studio-code"
      "xquartz"
      "yubico-authenticator"
      "zed"
    ];

    masApps = { };
  };
}

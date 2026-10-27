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
      # Homebrew は既定（`HOMEBREW_UPGRADE_AUTO_UPDATES_CASKS`）で、`auto_updates true` の cask も app bundle の版が
      # tap より古ければ greedy 指定なしで upgrade 対象にする（`brew outdated --cask` に notion 7.31.0 != 7.33.0 が
      # 出る）。`greedy = false` の cask をその cask 自身の updater へ委ねるため、この既定を切る。greedy upgrade
      # （`greedyCasks` / `greedy = true`）には影響しない。
      extraEnv = {
        HOMEBREW_NO_UPGRADE_AUTO_UPDATES_CASKS = "1";
      };
    };

    # `auto_updates true` / `version :latest` の cask も含め、個別に `greedy = false` を付けた cask を除く全 cask を
    # tap pin 追従の無人 upgrade 対象にする。前提となる「全 cask が sha256 固定」の受容根拠と強制機構は
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
      # self-updater（`agy update`）が Homebrew の binary symlink `/opt/homebrew/bin/agy` を実ファイルへ
      # 置き換える。Homebrew はこの cask を未インストールと見ており（Caskroom は `.metadata` のみ）、
      # `brew bundle` は `greedy` に関わらず `brew install --cask --adopt` を実行するが、`--adopt` は symlink 以外が
      # 占める target を上書きしないため、install は実ファイルの `/opt/homebrew/bin/agy` に当たって
      # 「It seems there is already a Binary at」で失敗する。この install 失敗はリポジトリ差分では解消せず、
      # `/opt/homebrew/bin/agy` の撤去で解消する。`greedy = false` は、再導入後に self-updater が symlink を
      # 実ファイルへ置き換えた際の再 upgrade 試行を止める。版の前進は self-updater に委ねる。
      {
        name = "antigravity-cli";
        greedy = false;
      }
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
      # pkg cask で、upgrade は旧版の uninstall script（`sudo: true`）と `installer` を `sudo` で走らせる。
      # auto-update daemon の `brew bundle` は root から Homebrew ユーザーへ降格した非対話実行で password を
      # 渡せず、16.1.0 からの greedy upgrade は `sudo: a password is required` で止まり続けた。版の前進は
      # Karabiner 自身の updater に委ね、無人 upgrade から外す。
      {
        name = "karabiner-elements";
        greedy = false;
      }
      "kicad"
      # `auto_updates true` の cask で、Notion 自身の updater が `/Applications/Notion.app` を差し替える。greedy
      # upgrade は root 所有の `/Applications/Notion.app` への `sudo` で password を要求して止まり、その後は
      # Caskroom に残った旧版との衝突「already an App at '/opt/homebrew/Caskroom/notion/7.31.0/Notion.app'」で
      # 止まり続けた。`greedy = false` と `onActivation.extraEnv` の `HOMEBREW_NO_UPGRADE_AUTO_UPDATES_CASKS` で
      # `brew bundle` は `Using notion` で通る。版の前進は Notion 自身の updater に委ね、無人 upgrade から外す。
      {
        name = "notion";
        greedy = false;
      }
      "slack"
      "visual-studio-code"
      "xquartz"
      "yubico-authenticator"
      "zed"
    ];

    masApps = { };
  };
}

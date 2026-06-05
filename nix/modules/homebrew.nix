# nix-darwin の `homebrew` option に渡す Homebrew 宣言。
#
# `taps` は brew bundle が参照する tap 名、`casks` は switch 時に導入する cask、
# `onActivation` は switch 時の更新と cleanup の扱いを指定する。
{ homebrewTaps, ... }:
{
  homebrew = {
    enable = true;
    taps = map (tap: tap.brewTap) homebrewTaps;

    onActivation = {
      # auto-update（launchd daemon + `dotfiles update`）が tap rev の pin に installed を追従させるため、
      # switch 時に `brew upgrade` 相当を実行する。pin は tap input の rev で決まり nightly bump で動くので、
      # 各マシンが実機の cask/formula を fleet の pin へ収束させる（利用者可視の挙動変更）。
      #
      # 残留リスク（セキュリティ）: `sha256` 未指定 / `auto_updates true` の cask は、ダウンロード成果物が
      # tap rev で固定されない。無人 upgrade で外部バイナリが無人差し替えされうる点を `autoUpdate=false`
      # では塞げない（tap 定義の更新有無の話であり、取得物の固定性とは別問題）。固定性が要る cask は手動更新へ
      # 寄せるか残留リスクとして扱う。`autoUpdate` は引き続き無効にして switch 時の暗黙 tap 取得を避ける。
      autoUpdate = false;
      upgrade = true;
      cleanup = "uninstall";
    };

    casks = [
      "azookey"
      "bitwarden"
      "codex-app"
      "font-cica"
      "ghostty"
      "yubico-authenticator"
    ];

    masApps = { };
  };
}

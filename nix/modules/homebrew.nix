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
      # switch 時に `brew upgrade` を実行する。pin は tap input の rev で決まり nightly bump で動くので、各
      # マシンが実機の cask/formula を fleet の pin へ収束させる（利用者可視の挙動変更）。
      #
      # 全 cask の成果物固定と greedy 有効化:
      #   tap rev は cask の「定義」を固定し、成果物の固定性は cask 側の `sha256` 指定に依存する。現宣言 cask は
      #   `auto_updates true` のものも含め全て `sha256` で成果物を明示固定しているため（owner 確認済み・明示受容。
      #   詳細は README「Homebrew cask の固定状況」）、greedy を有効化しても無人 upgrade 経路が差し替える成果物は
      #   tap rev で再現的に固定される。greedy 無効だと `brew upgrade` は `auto_updates true` / `version :latest`
      #   の cask を対象から除外し、これら（bitwarden / codex-app / ghostty）が tap pin へ決定論的に収束せず
      #   `dotfiles` の更新履歴にも出ないため、`greedyCasks = true` で全 cask を pin 追従の対象にする。
      #
      #   安全ガードの前提＝「全 cask が sha256 固定」。`sha256 :no_check`（= `version :latest`）の cask が将来
      #   混入すると、greedy 有効下では未固定成果物が無人差し替えされる。これを防ぐため、tap rev の cask `.rb` に
      #   `sha256 :no_check` があれば fail-closed にする検査を `dotfiles update-history record` 経路（Rust 側 brew
      #   モジュール）で実行する（cask 追加時の確認手順は README「Homebrew cask の固定状況」を参照）。auto-update
      #   適用後は `dotfiles update` の要約（端末 / pending-summary）が当該更新を利用者へ通知するため、無人で cask
      #   が上がった事実は可視化される。
      #
      # `autoUpdate` は引き続き無効にして switch 時の暗黙 tap 取得を避ける（tap は flake input rev で固定）。
      autoUpdate = false;
      upgrade = true;
      cleanup = "uninstall";
    };

    # 全 cask を tap pin 追従の無人 upgrade 対象にする（`auto_updates true` / `version :latest` も含む）。
    # 前提は「全 cask が sha256 固定」で、未固定成果物の無人差し替えは Rust 側 brew モジュールの `sha256 :no_check`
    # 検査が fail-closed で阻む。
    greedyCasks = true;

    # 宣言 cask。各 cask の sha256 固定 / auto_updates 状況は上の onActivation コメントと README
    # 「Homebrew cask の固定状況」で明示受容する。
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

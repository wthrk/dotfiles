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
      # switch 時に `brew upgrade`（greedy 無し）を実行する。pin は tap input の rev で決まり nightly bump で
      # 動くので、各マシンが実機の cask/formula を fleet の pin へ収束させる（利用者可視の挙動変更）。
      #
      # 未固定 cask 成果物の無人差し替えに対する緩和（セキュリティレビュー要修正1）:
      #   tap rev は cask の「定義」を固定するが、成果物の固定性は cask 側の `sha256` 指定に依存し、
      #   `sha256 :no_check`（= `version :latest`）や `auto_updates true` の cask は成果物が tap rev で
      #   固定されない。これに対する実緩和は **greedy を有効にしないこと**である。`brew upgrade` は既定で
      #   `auto_updates true` / `version :latest` の cask を**対象から除外**する（`--greedy` を渡したときだけ
      #   対象化する）。nix-darwin の `onActivation.upgrade=true` は `--greedy` を渡さないため、これら自己更新
      #   cask は無人 upgrade 経路の対象にならない（=本モジュールが外部成果物を無人差し替えしない）。自己更新
      #   cask はアプリ自身の更新機構で更新され、それは本設定の有無に関わらず動くため、本モジュールの責務外。
      #   よって無人 upgrade 経路が実際に成果物を差し替えるのは `sha256` が明示固定された cask に限られ、
      #   その成果物は tap rev で再現的に固定される。
      #
      #   現宣言 cask の素性（owner 確認済み、固定状況を明示受容。詳細は README「Homebrew cask の固定状況」）:
      #     - sha256 固定 + auto_updates 無し（無人 upgrade 対象・成果物固定で安全）: azookey / font-cica /
      #       yubico-authenticator。
      #     - auto_updates true（無人 upgrade 対象外・アプリ自己更新。sha256 は別途固定）: bitwarden /
      #       codex-app / ghostty。
      #   `sha256 :no_check` の未固定成果物を無人 upgrade する cask は現状存在しない。将来そうした cask を
      #   足す場合は、greedy を有効化しない現状では auto_updates 経由でのみ更新され（=本経路の対象外）、明示
      #   固定が要るなら手動更新へ寄せる。auto-update 適用後は `dotfiles update` の要約（端末 / pending-summary）
      #   が当該更新を利用者へ通知するため、無人で cask が上がった事実は可視化される。
      #
      # `autoUpdate` は引き続き無効にして switch 時の暗黙 tap 取得を避ける（tap は flake input rev で固定）。
      autoUpdate = false;
      upgrade = true;
      cleanup = "uninstall";
    };

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

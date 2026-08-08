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
      #   詳細は docs/automation/homebrew-cask-pinning.md）、greedy を有効化しても無人 upgrade 経路が差し替える成果物は
      #   tap rev で再現的に固定される。greedy 無効だと `brew upgrade` は `auto_updates true` / `version :latest`
      #   の cask を対象から除外し、これら（bitwarden / codex-app / ghostty）が tap pin へ決定論的に収束せず
      #   `dotfiles` の更新履歴にも出ないため、`greedyCasks = true` で全 cask を pin 追従の対象にする。
      #
      #   安全ガードの前提＝「全 cask が sha256 固定」。`sha256 :no_check`（= `version :latest`）の cask が将来
      #   混入すると、greedy 有効下では未固定成果物が無人差し替えされる。これを防ぐため、tap rev の cask `.rb` に
      #   `sha256 :no_check` があれば fail-closed にする検査を `dotfiles update-history record` 経路（Rust 側 brew
      #   モジュール）で実行する（cask 追加時の確認手順は docs/automation/homebrew-cask-pinning.md を参照）。auto-update
      #   適用後の変更内容は `dotfiles update-history show` で確認できるため、無人で cask が上がった事実は可視化される。
      #
      # `autoUpdate` は引き続き無効にして switch 時の暗黙 tap 取得を避ける（tap は flake input rev で固定）。
      autoUpdate = false;
      upgrade = true;
      # 宣言外パッケージをアンインストールする。nix-darwin はこの値のとき brew bundle へ --force-cleanup を渡す。
      #
      # 一時期 brew-src が 5.1.1 に据え置かれ、当時の brew に --force-cleanup が無かったため
      # cleanup = "none" + extraFlags = [ "--cleanup" ] で迂回していた（PR #87）。brew 6.0.13 では
      # --force-cleanup が存在し、逆に素の --cleanup は odeprecated かつ `--force` / `--force-cleanup` /
      # $HOMEBREW_ASK のいずれも無いと UsageError で停止する（Library/Homebrew/bundle/subcommand/install.rb）。
      # nightly が brew-src を含む全 input を bump するようになったため、迂回を戻して nix-darwin の
      # 生成する --force-cleanup をそのまま使う。
      #
      # この値は「lock 済み brew が --force-cleanup を持つこと」に依存する。nightly の verify-bump-lock は
      # 推移 input の ref 差分を方向を問わず通すため、下限を割る方向の bump を止めるのは
      # `cargo xtask check static` の homebrew_cleanup_matches_locked_brew_capability（brew-src の ref 下限を
      # flake.lock 上で強制する静的検査）である。受容条件は docs/automation/nightly-lock-bump.md の
      # 「残留制約」節を参照する。
      cleanup = "uninstall";
    };

    # 全 cask を tap pin 追従の無人 upgrade 対象にする（`auto_updates true` / `version :latest` も含む）。
    # 前提は「全 cask が sha256 固定」で、未固定成果物の無人差し替えは Rust 側 brew モジュールの `sha256 :no_check`
    # 検査が fail-closed で阻む。
    greedyCasks = true;

    # 宣言 cask。各 cask の sha256 固定 / auto_updates 状況は上の onActivation コメントと
    # docs/automation/homebrew-cask-pinning.md で明示受容する。
    #
    # `kicad` は nixpkgs 側（`pkgs/by-name/ki/kicad`）が `broken = stdenv.hostPlatform.isDarwin` のため
    # macOS では評価に通らず、cask で宣言する。cask `.rb` は `sha256` 固定・`auto_updates` 無しで、
    # greedy 前提（全 cask sha256 固定）を崩さない。nixpkgs と cask のどちらで宣言するかの規約は
    # docs/automation/homebrew-cask-pinning.md を正本とする。
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

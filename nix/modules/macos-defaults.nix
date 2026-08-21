# nix-darwin で固定する macOS の設定。
#
# Dock、Finder、キーリピートのように、ユーザー操作で変わりやすいがホスト全体で揃えたい値は
# `system.defaults` で宣言し、rebuild ごとに同じ状態へ戻す。Spotlight の索引はこの形で扱えないため、
# activation から `mdutil` を呼んで止める。索引データを消す片方向の操作であり、宣言を消しても
# 索引は戻らない（復旧手順は README にある）。
{ lib, ... }:
{
  system.defaults = {
    dock.autohide = true;
    finder.AppleShowAllExtensions = true;
    NSGlobalDomain = {
      AppleShowAllExtensions = true;
      InitialKeyRepeat = 15;
      KeyRepeat = 2;
    };
  };

  # Spotlight の索引を止め、既存の索引を消す。この設定だけ `system.defaults` ではなく activation から当てる。
  # 索引の on/off は preference domain ではなく各ボリュームの `/.Spotlight-V100/VolumeConfiguration.plist`
  # にあり、SIP 下では `defaults` から書けない。nix-darwin の option にも Apple の MDM payload にも索引を
  # 止める手段は無く、root で走る activation から Apple 提供の `mdutil` を呼ぶ形だけが宣言で完結する。
  system.activationScripts.postActivation.text =
    let
      # 起動ボリュームグループの 2 ストア。索引の状態はストアごとに独立している。`mdutil -a` は接続中の
      # 全ボリュームに当たり、外付けドライブやネットワーク共有の索引設定まで書き換えるため使わない。
      bootVolumeStores = [
        "/"
        "/System/Volumes/Data"
      ];
      # `-E` は索引が有効なままだと消したストアを mds が作り直す（`man mdutil`: "The stores will be
      # rebuilt if appropriate."）。消すだけで終わるのは索引を止めたボリュームに限られるので、
      # `-i off` が成功した場合にだけ `-E` を当てる。索引停止は本体の適用より優先度が低いので、
      # 失敗しても activation を止めず、事実だけを残す。
      stopIndexing = store: ''
        if mdutil -i off ${store}; then
          mdutil -E ${store} || echo "Spotlight: mdutil -E ${store} に失敗しました" >&2
        else
          echo "Spotlight: mdutil -i off ${store} に失敗しました" >&2
        fi
      '';
    in
    lib.concatMapStrings stopIndexing bootVolumeStores;
}

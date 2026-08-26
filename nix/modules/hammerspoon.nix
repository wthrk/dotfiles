# Hammerspoon を起動する launch agent を宣言する。配置と起動の扱いは
# `nix/modules/launch-agents.nix` が持つ。
#
# 入力ソースを ABC へ戻す処理は `config/hammerspoon/init.lua`（`shell-files.nix` がリンク）にあり、
# Hammerspoon 本体は Homebrew cask で入る（`homebrew.nix`）。init.lua は Hammerspoon が動いて
# いなければ読まれないので、起動もここで宣言する。
#
# nix-darwin の `launchd.user.agents` ではなく home 層に置く。前者が配置するのは `system.primaryUser`
# の分だけで、init.lua をリンクする `shell-files.nix` は Home Manager 側にある。起動とリンクで対象
# ユーザーを揃える。
#
# 起動条件は `RunAtLoad`（ログイン）と `WatchPaths`（cask がアプリを置いた時点）の 2 つを持つ。適用は
# home 層 -> system 層の順に走るので、この launch agent を bootstrap する時点では cask が未導入で `open`
# が失敗する。cask の到着を `WatchPaths` で受けることで初回適用でも起動する。`open -g` は起動したアプリ
# を前面へ出さないので、操作中のウィンドウからフォーカスを奪わない。
{ lib, pkgs, ... }:
let
  bundlePath = "/Applications/Hammerspoon.app";
in
{
  dotfiles.launchAgents = lib.mkIf pkgs.stdenv.hostPlatform.isDarwin {
    "org.nix.hammerspoon" = {
      ProgramArguments = [
        "/usr/bin/open"
        "-g"
        "-a"
        bundlePath
      ];
      RunAtLoad = true;
      WatchPaths = [ bundlePath ];
    };
  };
}

# GUI エディタが期待する設定ディレクトリを Home Manager activation で用意する。
#
# 用意するのは設定ディレクトリだけで、その内容の所有権はアプリに残す。作るのは空ディレクトリ
# なので再実行しても既存設定を破壊しない。エディタ本体は `nix/modules/homebrew.nix` が cask で宣言する。
{ lib, ... }:
{
  # Zed が初回起動時に書き込む場所を先に作る。
  home.activation.editorPolicy = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    mkdir -p "$HOME/.config/zed"
  '';
}

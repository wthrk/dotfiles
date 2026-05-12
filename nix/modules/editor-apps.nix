# GUI エディタが期待する設定ディレクトリを Home Manager activation で用意する。
#
# エディタ本体や認証情報は Home Manager では固定しない。再実行しても既存設定を破壊しない
# 空ディレクトリだけを作る。
{ lib, ... }:
{
  # Zed などが初回起動時に書き込む場所だけを先に作り、内容の所有権はアプリに残す。
  home.activation.editorPolicy = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    mkdir -p "$HOME/.config/zed"
  '';
}

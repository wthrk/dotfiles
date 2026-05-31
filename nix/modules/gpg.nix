# gpg-agent の SSH support 設定を Home Manager 管理下で恒久化する。
#
# `dotfiles secrets restore-gpg` は GPG authentication subkey を SSH identity として使うため、
# gpg-agent の SSH support が有効である前提を必要とする。鍵ごとの登録状態（sshcontrol）は
# `restore-gpg` 実装側が管理し、このモジュールは `gpg-agent.conf` の静的設定だけを管理する。
# 利用者が手動で `~/.gnupg/gpg-agent.conf` を編集して状態を分岐させる運用は採用しない。
{
  lib,
  pkgs,
  ...
}:
let
  # macOS で利用する pinentry 実体。`cli.nix` が `pinentry_mac` を packages に入れる前提に揃える。
  pinentryProgram =
    if pkgs.stdenv.isDarwin && (lib.hasAttrByPath [ "pinentry_mac" ] pkgs) then
      "${pkgs.pinentry_mac}/Applications/pinentry-mac.app/Contents/MacOS/pinentry-mac"
    else
      "${pkgs.pinentry}/bin/pinentry";
in
{
  # gpg-agent.conf を生成し、SSH support と pinentry を恒久設定する。
  # `enable-ssh-support` により gpg-agent が `S.gpg-agent.ssh` socket を提供し、
  # GPG authentication subkey を OpenSSH agent 経路で利用できるようにする。
  home.file.".gnupg/gpg-agent.conf".text = ''
    enable-ssh-support
    pinentry-program ${pinentryProgram}
  '';
}

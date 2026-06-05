# nightly bump 後に各マシンを repo pin へ無人で収束させる auto-update daemon。
#
# nix-darwin ネイティブの `launchd.daemons`（root/system。`launchagents`＝user ではない）として
# `StartCalendarInterval` で定期起動し、権限分割ラッパー経由で `dotfiles` CLI を呼ぶ。ラッパーは
# 権限分割のみを担う薄い層で、適用要否判定・home-manager 適用・要約/マーカー書込みといった業務判断は
# すべて `dotfiles` CLI 側にあり、nix へは漏らさない。
#
# 権限分割（最重要）:
#   - home-manager 適用とユーザ状態（`last-applied-rev` / `pending-summary` / `last-run.log` /
#     `update.lock`）の書込みは `sudo -u <user> dotfiles update <user-targets>` でユーザ権限で行う。
#     home-manager を root で走らせず、状態マーカーをユーザ所有で書くことを厳守する。
#   - darwin-rebuild は root のまま `DOTFILES_DARWIN_REBUILD_SUDO=0 dotfiles switch darwin` で適用する
#     （既に root のため sudo を前置しない経路）。
#
# rev マーカー確定のタイミング（drift 防止）:
#   home と darwin は別 CLI 起動で適用するため、home ステップでは `--defer-rev-marker` を付けて
#   `last-applied-rev` をまだ書かない。home+darwin の両適用が成功した後にだけ `--commit-rev-marker` で
#   rev を確定する。darwin が失敗すると（`set -e`）確定ステップへ到達せず、マーカーは前回値のまま残るので、
#   次回起動で再適用して収束する（darwin 未収束のまま「適用済み」と誤記録して skip する drift を防ぐ）。
#
# `~/.local/state/dotfiles` は root で mkdir せず、system.activationScripts で `launchctl asuser` +
# `sudo -u <user>` によりユーザ所有で作成する（launchagents.nix の asuser idiom に倣う）。
{
  user,
  pkgs,
  lib,
  inputs,
  ...
}:
let
  label = "org.dotfiles.auto-update";
  homeDir = "/Users/${user}";
  stateDir = "${homeDir}/.local/state/dotfiles";

  system = pkgs.stdenv.hostPlatform.system;
  # 絶対 store パスで CLI を指す（PATH 非依存。root daemon の最小環境で確実に解決するため）。
  dotfilesBin = "${inputs.self.packages.${system}.default}/bin/dotfiles";

  # 権限分割のみを担う薄いラッパー。業務判断は持たず、誰の権限でどのサブコマンドを呼ぶかだけを決める。
  wrapper = pkgs.writeShellScript "${label}-wrapper" ''
    set -euo pipefail

    # ① state dir をユーザ所有で用意（activation でも作るが、daemon 起動時の取りこぼしに備える）。
    #    root では mkdir せず、ユーザ権限で作る（マーカーのユーザ所有保証）。
    /usr/bin/sudo -u ${lib.escapeShellArg user} /bin/mkdir -p ${lib.escapeShellArg stateDir}

    # ② 適用要否判定・home-manager 適用・要約書込みをユーザ権限で実行する。非 tty なので要約は
    #    pending-summary へ書かれる。darwin は別ステップ（root）で適用するため、ここでは home のみを対象に
    #    する（home-manager を root で走らせない）。`--defer-rev-marker` で `last-applied-rev` はまだ書かず、
    #    home+darwin の両成功後（④）に確定する。これにより darwin 失敗時の rev drift を防ぐ。
    /usr/bin/sudo -u ${lib.escapeShellArg user} ${dotfilesBin} update home --defer-rev-marker

    # ③ darwin 部分は root のまま、sudo を前置しない経路で適用する（既に root daemon のため）。
    DOTFILES_DARWIN_REBUILD_SUDO=0 ${dotfilesBin} switch darwin --no-sudo

    # ④ home+darwin の両適用が成功した後にだけ rev マーカーを確定する。set -e により②③のいずれかが失敗
    #    すると④へ到達せず、`last-applied-rev` は前回値のまま残る（次回再適用で収束させ、drift を回避）。
    #    マーカーはユーザ所有で書くため、確定もユーザ権限で行う。
    /usr/bin/sudo -u ${lib.escapeShellArg user} ${dotfilesBin} update home --commit-rev-marker
  '';
in
{
  launchd.daemons.${label} = {
    serviceConfig = {
      Label = label;
      # ProgramArguments は権限分割ラッパーそのもの（薄い層）。業務判断は dotfiles CLI 側に置く。
      ProgramArguments = [ "${wrapper}" ];
      # nightly bump（CI の自動マージ）後に収束させる時刻。ログイン前提を持たないので毎日 09:00。
      StartCalendarInterval = [
        {
          Hour = 9;
          Minute = 0;
        }
      ];
      # 起動時の即時実行はしない（ブート直後の無人適用を避け、定期発火のみで動かす）。
      RunAtLoad = false;
      StandardOutPath = "/var/log/${label}.out.log";
      StandardErrorPath = "/var/log/${label}.err.log";
    };
  };

  # `~/.local/state/dotfiles` をユーザ所有で作成する。root で mkdir すると root 所有になり、daemon の
  # `sudo -u <user>` 書込みやシェルからの consume が壊れるため、launchctl asuser + sudo -u でユーザ権限で作る。
  system.activationScripts.postActivation.text = lib.mkAfter ''
    uid="$(id -u ${lib.escapeShellArg user})"
    launchctl asuser "$uid" sudo --user=${lib.escapeShellArg user} -- mkdir -p ${lib.escapeShellArg stateDir}
  '';
}

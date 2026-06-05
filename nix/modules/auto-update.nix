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
# 無更新日の darwin-rebuild 抑止（F3）:
#   home ステップが「適用済み pin と同一で no-op」だった日は darwin-rebuild を起動しない。home 適用前の
#   `last-applied-rev` を控え、home ステップが lock を更新した後の dotfiles pin と突き合わせ、変化が無ければ
#   darwin ステップと rev 確定を skip する。`--defer-rev-marker` のため `last-applied-rev` は前回値のまま
#   なので、この比較で「実際に適用したか」を判別できる。
#
# launchd PATH（F1）:
#   launchd daemon の最小 PATH には nix が無い。`dotfiles` は絶対 store パスで起動するが、内部で `nix`・
#   `home-manager`・`darwin-rebuild` を PATH 解決で spawn するため、wrapper で PATH を明示的に通す。sudo は
#   env をリセットするので、ユーザ権限ステップは `env PATH="$PATH"` を前置して PATH を引き継ぐ。
#
# `~/.local/state/dotfiles` は root で mkdir せず、system.activationScripts で `launchctl asuser` +
# `sudo -u <user>` によりユーザ所有で作成する（launchagents.nix の asuser idiom に倣う）。
{
  user,
  pkgs,
  lib,
  inputs,
  config,
  ...
}:
let
  label = "org.dotfiles.auto-update";
  homeDir = "/Users/${user}";
  stateDir = "${homeDir}/.local/state/dotfiles";
  # 利用者の dotfiles ローカル flake（`dotfiles init` が書く `~/.config/dotfiles`）。root daemon の darwin 適用は
  # root で走るため、`--config-dir` を渡さないと `$HOME`（/var/root）配下の存在しない config を見に行き失敗する。
  # home/darwin 双方にこのユーザ config を明示し、root と user で同じローカル flake を確実に指す。
  configDir = "${homeDir}/.config/dotfiles";

  system = pkgs.stdenv.hostPlatform.system;
  # 絶対 store パスで CLI を指す（PATH 非依存。root daemon の最小環境で確実に解決するため）。
  dotfilesBin = "${inputs.self.packages.${system}.default}/bin/dotfiles";

  # launchd daemon の最小 PATH には nix が無い。`dotfiles` バイナリ自体は絶対 store パスで起動できるが、
  # `dotfiles update home` は内部で `nix`（`nix flake update`）と `home-manager` を、`dotfiles switch darwin` は
  # `darwin-rebuild` を **PATH 解決で** spawn する。これらが見つからないと daemon が失敗するため、wrapper の
  # PATH を明示的に通す。store パスで確実に解決できる `nix`（`config.nix.package`）と coreutils（`mkdir`/`mv`
  # 等の退避処理）を makeBinPath で先頭に置き、nix-darwin/home-manager が提供する `darwin-rebuild`・
  # `home-manager` は実行時 profile（`/run/current-system/sw/bin`・`/etc/profiles/per-user/<user>/bin`・
  # nix default profile）から解決する（これらは build 時の store パスに固定できないため runtime profile で引く）。
  daemonPath = lib.concatStringsSep ":" [
    (lib.makeBinPath [
      config.nix.package
      pkgs.coreutils
      # 無更新日に darwin-rebuild を起動しない判定（F3）で flake.lock の pin を厳密抽出するため jq を通す。
      pkgs.jq
    ])
    "/run/current-system/sw/bin"
    "/etc/profiles/per-user/${user}/bin"
    "/nix/var/nix/profiles/default/bin"
    "/usr/bin"
    "/bin"
    "/usr/sbin"
    "/sbin"
  ];

  # 権限分割のみを担う薄いラッパー。業務判断は持たず、誰の権限でどのサブコマンドを呼ぶかだけを決める。
  wrapper = pkgs.writeShellScript "${label}-wrapper" ''
    set -euo pipefail

    # launchd daemon の最小 PATH には nix が無い。wrapper 自身（root の darwin ステップが PATH 解決で
    # `darwin-rebuild` を引く）と、`sudo -u <user> env PATH=... dotfiles ...` 経由で渡すユーザ権限ステップの
    # 双方で nix / home-manager / darwin-rebuild を解決できるよう PATH を確定する。sudo は既定で env を
    # リセットし secure_path で PATH を上書きしうるため、ユーザ権限ステップでは `env PATH="$PATH"` を前置して
    # 明示的に引き継ぐ。
    export PATH=${lib.escapeShellArg daemonPath}

    state_dir=${lib.escapeShellArg stateDir}
    config_dir=${lib.escapeShellArg configDir}

    # ① state dir をユーザ所有で用意（activation でも作るが、daemon 起動時の取りこぼしに備える）。
    #    root では mkdir せず、ユーザ権限で作る（マーカーのユーザ所有保証）。
    /usr/bin/sudo -u ${lib.escapeShellArg user} /bin/mkdir -p "$state_dir"

    # F3: home ステップが実際に適用したか（repo pin が変化したか）を判定するため、home 適用「前」の
    #     `last-applied-rev` を控える。home ステップは `--defer-rev-marker` で `last-applied-rev` を書かないので、
    #     適用後にローカル lock の dotfiles pin と突き合わせれば「適用済み pin と同一＝無更新」を判別できる。
    applied_before=""
    if [ -r "$state_dir/last-applied-rev" ]; then
      applied_before="$(tr -d '[:space:]' < "$state_dir/last-applied-rev" || true)"
    fi

    # ② 適用要否判定・home-manager 適用・要約書込みをユーザ権限で実行する。非 tty なので要約は
    #    pending-summary へ書かれる。darwin は別ステップ（root）で適用するため、ここでは home のみを対象に
    #    する（home-manager を root で走らせない）。`--defer-rev-marker` で `last-applied-rev` はまだ書かず、
    #    home+darwin の両成功後（④）に確定する。これにより darwin 失敗時の rev drift を防ぐ。`--config-dir` で
    #    ユーザの `~/.config/dotfiles` を明示し、確実にユーザのローカル flake を指す。sudo は env をリセットし
    #    PATH を secure_path で上書きしうるため、`env PATH="$PATH"` を前置して nix / home-manager を解決させる。
    /usr/bin/sudo -u ${lib.escapeShellArg user} /usr/bin/env PATH="$PATH" ${dotfilesBin} update home \
      --config-dir "$config_dir" --defer-rev-marker

    # F3: home ステップが lock を最新 repo pin へ更新した後の pin を読む。`--defer-rev-marker` のため
    #     `last-applied-rev` はまだ前回値（applied_before）のまま。新 pin が前回値と同一なら home は no-op
    #     （適用なし）であり、darwin-rebuild を起動する理由が無いので darwin ステップと rev 確定を skip する
    #     （無更新日に darwin-rebuild を走らせない）。pin が変化していれば実適用があったので darwin を適用する。
    applied_after="$(jq -r '.nodes.dotfiles.locked.rev // empty' "$config_dir/flake.lock" 2>/dev/null || true)"
    if [ -n "$applied_after" ] && [ "$applied_after" = "$applied_before" ]; then
      echo "home に更新がないため darwin-rebuild を skip します（rev $applied_after）"
      exit 0
    fi

    # ③ darwin 部分は root のまま、sudo を前置しない経路で適用する（既に root daemon のため）。root では
    #    `$HOME` が /var/root のため `--config-dir` でユーザ config を明示しないと存在しない config を見に行く。
    #    PATH は wrapper の export 済み値を継承し、`darwin-rebuild` を runtime profile から解決する。
    DOTFILES_DARWIN_REBUILD_SUDO=0 ${dotfilesBin} switch darwin \
      --config-dir "$config_dir" --no-sudo

    # ④ home+darwin の両適用が成功した後にだけ rev マーカーを確定する。set -e により②③のいずれかが失敗
    #    すると④へ到達せず、`last-applied-rev` は前回値のまま残る（次回再適用で収束させ、drift を回避）。
    #    マーカーはユーザ所有で書くため、確定もユーザ権限で行う。`--config-dir` で読む pin を②と一致させる。
    /usr/bin/sudo -u ${lib.escapeShellArg user} /usr/bin/env PATH="$PATH" ${dotfilesBin} update home \
      --config-dir "$config_dir" --commit-rev-marker
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

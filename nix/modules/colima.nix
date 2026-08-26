# Colima 用 launch agent を宣言する。配置と起動の扱いは `nix/modules/launch-agents.nix` が持つ。
#
# 既存の Homebrew 由来 plist は同じサービスを二重起動しないよう撤去する。ログの出力先は launchd が
# 親ディレクトリを作らないので、agent を起動する前に用意する。colima 本体は `nix/modules/cli.nix` の
# `home.packages` で入る。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  label = "org.nix.colima";
  oldLabel = "homebrew.mxcl.colima";
  homeDir = config.home.homeDirectory;
  logDir = "${homeDir}/Library/Logs";
  oldPlistPath = "${homeDir}/Library/LaunchAgents/${oldLabel}.plist";
in
{
  dotfiles.launchAgents = lib.mkIf pkgs.stdenv.hostPlatform.isDarwin {
    "${label}" = {
      EnvironmentVariables.PATH = "$HOME/.nix-profile/bin:/etc/profiles/per-user/$USER/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";
      KeepAlive = true;
      ProcessType = "Interactive";
      ProgramArguments = [
        "${pkgs.colima}/bin/colima"
        "start"
        "-f"
      ];
      RunAtLoad = true;
      StandardErrorPath = "${logDir}/${label}.err.log";
      StandardOutPath = "${logDir}/${label}.out.log";
    };
  };

  home.activation.replaceHomebrewColimaLaunchAgent = lib.mkIf pkgs.stdenv.hostPlatform.isDarwin (
    lib.hm.dag.entryBefore [ "loadLaunchAgents" ] ''
      /bin/mkdir -p "${logDir}"

      /bin/launchctl bootout "gui/$(id -u)/${oldLabel}" >/dev/null 2>&1 || true
      /bin/rm -f "${oldPlistPath}"
    ''
  );
}

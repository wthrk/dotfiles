# Colima 用 launch agent を対象ユーザーの `~/Library/LaunchAgents` に配置する。
#
# 引数 `user` からホームディレクトリと plist の配置先を作る。既存の Homebrew 由来 plist は
# 同じサービスを二重起動しないよう退避し、activation 時に launchctl の登録状態を揃える。
{
  lib,
  pkgs,
  user,
  ...
}:
let
  label = "org.nix.colima";
  oldLabel = "homebrew.mxcl.colima";
  homeDir = "/Users/${user}";
  plistPath = "${homeDir}/Library/LaunchAgents/${label}.plist";
  oldPlistPath = "${homeDir}/Library/LaunchAgents/${oldLabel}.plist";
  activationStatePath = "${homeDir}/.local/state/nix/${label}.activation-mode";
  plistFile = pkgs.writeText "${label}.plist" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
      "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <dict>
      <key>EnvironmentVariables</key>
      <dict>
        <key>PATH</key>
        <string>$HOME/.nix-profile/bin:/etc/profiles/per-user/$USER/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
      </dict>
      <key>KeepAlive</key>
      <true/>
      <key>Label</key>
      <string>${label}</string>
      <key>ProcessType</key>
      <string>Interactive</string>
      <key>ProgramArguments</key>
      <array>
        <string>${pkgs.colima}/bin/colima</string>
        <string>start</string>
        <string>-f</string>
      </array>
      <key>RunAtLoad</key>
      <true/>
      <key>StandardErrorPath</key>
      <string>${homeDir}/Library/Logs/${label}.err.log</string>
      <key>StandardOutPath</key>
      <string>${homeDir}/Library/Logs/${label}.out.log</string>
    </dict>
    </plist>
  '';
in
{
  home-manager.users.${user} =
    { lib, ... }:
    {
      home.file."Library/LaunchAgents/${label}.plist" = {
        force = true;
        source = plistFile;
      };

      home.activation.removeDanglingColimaLaunchAgent = lib.hm.dag.entryBefore [ "checkLinkTargets" ] ''
        plist="${plistPath}"

        if [ -L "$plist" ] && [ ! -e "$plist" ]; then
          /bin/rm -f "$plist"
        fi
      '';

      home.activation.loadColimaLaunchAgent = lib.hm.dag.entryAfter [ "linkGeneration" ] ''
        plist="${plistPath}"
        state_file="${activationStatePath}"
        uid="$(id -u)"
        domain="gui/$uid"
        service="$domain/${label}"
        old_service="$domain/${oldLabel}"

        /bin/mkdir -p "${homeDir}/Library/Logs"
        /bin/mkdir -p "$(/usr/bin/dirname "$state_file")"

        if /bin/launchctl print "$domain" >/dev/null 2>&1; then
          if ! /bin/launchctl print "$service" >/dev/null 2>&1; then
            if ! /bin/launchctl bootstrap "$domain" "$plist" >/dev/null 2>&1; then
              /bin/sleep 1
              if ! /bin/launchctl print "$service" >/dev/null 2>&1; then
                /bin/launchctl bootstrap "$domain" "$plist"
              fi
            fi
          fi
          /bin/launchctl enable "$service" >/dev/null 2>&1 || true

          /bin/launchctl bootout "$old_service" >/dev/null 2>&1 || true
          /bin/rm -f "${oldPlistPath}"
          printf '%s\n' "loaded" > "$state_file"
        else
          /bin/rm -f "${oldPlistPath}"
          printf '%s\n' "installed-without-loading" > "$state_file"
          echo "$domain launchd domain is unavailable; installed ${label} plist without loading it"
        fi
      '';
    };

  system.activationScripts.postActivation.text = lib.mkAfter ''
    uid="$(id -u -- ${lib.escapeShellArg user})"

    launchctl asuser "$uid" sudo --user=${lib.escapeShellArg user} -- launchctl bootout "gui/$uid/${oldLabel}" >/dev/null 2>&1 || true
    sudo --user=${lib.escapeShellArg user} -- rm -f ${lib.escapeShellArg oldPlistPath}
    sudo --user=${lib.escapeShellArg user} -- rm -f ${lib.escapeShellArg activationStatePath}
  '';
}

{
  config,
  lib,
  pkgs,
  user,
  ...
}:
let
  label = "org.nix.colima";
  oldLabel = "homebrew.mxcl.colima";
  plistPath = "${config.home.homeDirectory}/Library/LaunchAgents/${label}.plist";
  oldPlistPath = "${config.home.homeDirectory}/Library/LaunchAgents/${oldLabel}.plist";
  plistFile = pkgs.writeText "${label}.plist" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
      "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <dict>
      <key>Label</key>
      <string>${label}</string>
      <key>ProgramArguments</key>
      <array>
        <string>${pkgs.colima}/bin/colima</string>
        <string>start</string>
        <string>-f</string>
      </array>
      <key>KeepAlive</key>
      <true/>
      <key>RunAtLoad</key>
      <true/>
      <key>StandardOutPath</key>
      <string>/Users/${user}/Library/Logs/${label}.out.log</string>
      <key>StandardErrorPath</key>
      <string>/Users/${user}/Library/Logs/${label}.err.log</string>
      <key>ProcessType</key>
      <string>Interactive</string>
    </dict>
    </plist>
  '';
in
{
  home.file."Library/LaunchAgents/${label}.plist".source = plistFile;

  home.activation.removeDanglingColimaLaunchAgent = lib.hm.dag.entryBefore [ "checkLinkTargets" ] ''
    plist="${plistPath}"

    if [ -L "$plist" ] && [ ! -e "$plist" ]; then
      /bin/rm -f "$plist"
    fi
  '';

  home.activation.loadColimaLaunchAgent = lib.hm.dag.entryAfter [ "linkGeneration" ] ''
    plist="${plistPath}"
    uid="$(id -u)"
    domain="gui/$uid"
    service="$domain/${label}"
    old_service="$domain/${oldLabel}"

    /bin/mkdir -p "${config.home.homeDirectory}/Library/Logs"

    if /bin/launchctl print "$domain" >/dev/null 2>&1; then
      if ! /bin/launchctl print "$service" >/dev/null 2>&1; then
        /bin/launchctl bootstrap "$domain" "$plist"
      fi
      /bin/launchctl enable "$service" >/dev/null 2>&1 || true

      /bin/launchctl bootout "$old_service" >/dev/null 2>&1 || true
      /bin/rm -f "${oldPlistPath}"
    else
      /bin/rm -f "${oldPlistPath}"
      echo "$domain launchd domain is unavailable; installed ${label} plist without loading it"
    fi
  '';
}

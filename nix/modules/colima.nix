# Colima 用 launch agent を `~/Library/LaunchAgents` に配置する。
#
# この宣言は home 層に置く。`nix/home.nix` から入るため `dotfiles switch home` 単独適用でも新世代の
# home-files に含まれ、Home Manager の `linkGeneration` が旧世代のリンクを orphan として撤去しない。
# darwin 層の import から `home-manager.users.<user>` へ差し込んでいた間は、home 層だけを適用すると
# `~/Library/LaunchAgents/org.nix.colima.plist` が実機から消えた。
#
# 配置先は `config.home.homeDirectory` から作る。既存の Homebrew 由来 plist は同じサービスを二重起動
# しないよう撤去し、activation 時に launchctl の登録状態を揃える。colima 本体は `nix/modules/cli.nix` の
# `home.packages` で入る。launchd と `~/Library/LaunchAgents` は macOS にしか無いため、plist の配置と
# activation は `mkIf pkgs.stdenv.isDarwin` で macOS だけに限る。
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
  plistPath = "${homeDir}/Library/LaunchAgents/${label}.plist";
  oldPlistPath = "${homeDir}/Library/LaunchAgents/${oldLabel}.plist";
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
  home.file."Library/LaunchAgents/${label}.plist" = lib.mkIf pkgs.stdenv.isDarwin {
    force = true;
    source = plistFile;
  };

  home.activation.removeDanglingColimaLaunchAgent = lib.mkIf pkgs.stdenv.isDarwin (
    lib.hm.dag.entryBefore [ "checkLinkTargets" ] ''
      plist="${plistPath}"

      if [ -L "$plist" ] && [ ! -e "$plist" ]; then
        /bin/rm -f "$plist"
      fi
    ''
  );

  home.activation.loadColimaLaunchAgent = lib.mkIf pkgs.stdenv.isDarwin (
    lib.hm.dag.entryAfter [ "linkGeneration" ] ''
      plist="${plistPath}"
      uid="$(id -u)"
      domain="gui/$uid"
      service="$domain/${label}"
      old_service="$domain/${oldLabel}"

      /bin/mkdir -p "${homeDir}/Library/Logs"

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
      else
        /bin/rm -f "${oldPlistPath}"
        echo "$domain launchd domain is unavailable; installed ${label} plist without loading it"
      fi
    ''
  );
}

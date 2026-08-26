# `~/Library/LaunchAgents` に置く launch agent を label ごとに宣言する。
#
# 宣言は home 層に置く。`nix/home.nix` から入るため `dotfiles switch home` 単独適用でも新世代の
# home-files に含まれ、Home Manager の `linkGeneration` が旧世代のリンクを orphan として撤去しない。
# darwin 層の import から `home-manager.users.<user>` へ差し込んでいた間は、home 層だけを適用すると
# `~/Library/LaunchAgents/org.nix.colima.plist` が実機から消えた。
#
# 起動は `gui/<uid>` domain がある場合だけ行う。Home Manager 標準の `launchd.agents` はこの domain へ
# 無条件に bootstrap するため、GUI ログインしたことのないユーザーでは `Bootstrap failed: 125: Domain
# does not support specified action` になり、`setupLaunchAgents` ごと activation が失敗する。runtime
# 統合検証がシナリオ内で作るユーザーへの適用はこれで止まった。ここでは `launchctl print` で domain の
# 有無を先に確かめ、無ければ plist を置くだけにして activation を続ける。
#
# launchd と `~/Library/LaunchAgents` は macOS にしか無いため、配置と起動は `pkgs.stdenv.hostPlatform.isDarwin` の
# ときだけ有効にする。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.launchAgents;
  labels = lib.attrNames cfg;
  agentDir = "${config.home.homeDirectory}/Library/LaunchAgents";

  # `Label` は属性名から入れる。plist の中身と launchctl へ渡す service 名の出所を 1 つにする。
  plistFile =
    label: settings:
    pkgs.writeText "${label}.plist" (
      lib.generators.toPlist { escape = true; } (settings // { Label = label; })
    );

  shellLabels = lib.escapeShellArgs labels;
in
{
  options.dotfiles.launchAgents = lib.mkOption {
    type = lib.types.attrsOf (lib.types.attrsOf lib.types.anything);
    default = { };
    example = lib.literalExpression ''
      {
        "org.nix.example" = {
          ProgramArguments = [ "/usr/bin/true" ];
          RunAtLoad = true;
        };
      }
    '';
    description = ''
      launch agent を label ごとに宣言する。値は `launchd.plist(5)` の内容で、`Label` は属性名から入る。
    '';
  };

  config = lib.mkIf (pkgs.stdenv.hostPlatform.isDarwin && cfg != { }) {
    home.file = lib.mapAttrs' (
      label: settings:
      lib.nameValuePair "Library/LaunchAgents/${label}.plist" {
        force = true;
        source = plistFile label settings;
      }
    ) cfg;

    home.activation.removeDanglingLaunchAgents = lib.hm.dag.entryBefore [ "checkLinkTargets" ] ''
      for label in ${shellLabels}; do
        plist="${agentDir}/$label.plist"
        if [ -L "$plist" ] && [ ! -e "$plist" ]; then
          /bin/rm -f "$plist"
        fi
      done
    '';

    # domain があるのに登録できなければ activation を失敗させる。plist を置いただけの状態を成功として報告しない。
    home.activation.loadLaunchAgents = lib.hm.dag.entryAfter [ "linkGeneration" ] ''
      domain="gui/$(id -u)"

      if /bin/launchctl print "$domain" >/dev/null 2>&1; then
        for label in ${shellLabels}; do
          service="$domain/$label"
          plist="${agentDir}/$label.plist"

          if ! /bin/launchctl print "$service" >/dev/null 2>&1; then
            if ! /bin/launchctl bootstrap "$domain" "$plist" >/dev/null 2>&1; then
              /bin/sleep 1
              if ! /bin/launchctl print "$service" >/dev/null 2>&1; then
                /bin/launchctl bootstrap "$domain" "$plist"
              fi
            fi
          fi
          /bin/launchctl enable "$service" >/dev/null 2>&1 || true
        done
      else
        echo "$domain launchd domain is unavailable; installed ${lib.escapeShellArg (lib.concatStringsSep ", " labels)} plist without loading it"
      fi
    '';
  };
}

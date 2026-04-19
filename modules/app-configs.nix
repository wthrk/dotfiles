{ lib, pkgs, ... }:
let
  composePluginCandidates = [
    "${pkgs.docker-compose}/libexec/docker/cli-plugins/docker-compose"
    "${pkgs.docker-compose}/bin/docker-compose"
  ];
in
{
  home.activation.appConfigDirs = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    mkdir -p "$HOME/.docker/cli-plugins"
    mkdir -p "$HOME/.aws/credentials.d"

    compose_target=""
    for candidate in ${lib.escapeShellArgs composePluginCandidates}; do
      if [ -x "$candidate" ]; then
        compose_target="$candidate"
        break
      fi
    done

    if [ -n "$compose_target" ]; then
      ln -sfn "$compose_target" "$HOME/.docker/cli-plugins/docker-compose"
    fi
  '';
}

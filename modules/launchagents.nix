{ pkgs, user, ... }:
{
  launchd.user.agents.org-nix-colima = {
    serviceConfig = {
      Label = "org.nix.colima";
      ProgramArguments = [ "${pkgs.colima}/bin/colima" "start" "-f" ];
      KeepAlive = true;
      RunAtLoad = true;
      StandardOutPath = "/Users/${user}/Library/Logs/org.nix.colima.out.log";
      StandardErrorPath = "/Users/${user}/Library/Logs/org.nix.colima.err.log";
      ProcessType = "Interactive";
    };
  };
}

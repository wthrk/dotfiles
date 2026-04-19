{ config, lib, ... }:
let
  cfg = config.dotfiles;
in
{
  options.dotfiles.enableSops = lib.mkEnableOption "Enable sops-nix secrets deployment";

  config = lib.mkIf cfg.enableSops {
    sops = {
      age.keyFile = "${config.xdg.configHome}/sops/age/keys.txt";
      defaultSopsFile = ../secrets/users/ya.yaml;
      secrets."aws/credentials_static" = {
        path = "${config.home.homeDirectory}/.aws/credentials.d/static";
        mode = "0400";
      };
    };
  };
}

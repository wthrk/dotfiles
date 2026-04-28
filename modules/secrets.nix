{
  config,
  lib,
  user,
  ...
}:
let
  cfg = config.dotfiles;
in
{
  options.dotfiles.enableSops = lib.mkEnableOption "Enable sops-nix secrets deployment";

  config = lib.mkIf cfg.enableSops (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = user == "ya";
            message = "dotfiles.enableSops は user=ya のみ対応しています。CI ユーザーでは有効化しないでください。";
          }
        ];
      }
      (lib.mkIf (user == "ya") {
        sops = {
          age.keyFile = "${config.xdg.configHome}/sops/age/keys.txt";
          defaultSopsFile = ../secrets/users/ya.yaml;
          secrets."aws/credentials_static" = {
            path = "${config.home.homeDirectory}/.aws/credentials.d/static";
            mode = "0400";
          };
        };
      })
    ]
  );
}

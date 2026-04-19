{ user, ... }:
{
  imports = [
    ./modules/cli.nix
    ./modules/languages.nix
    ./modules/zsh.nix
    ./modules/git.nix
    ./modules/neovim.nix
    ./modules/editor-apps.nix
    ./modules/shell-files.nix
    ./modules/app-configs.nix
    ./modules/secrets.nix
    ./modules/direnv.nix
  ];

  home.username = user;
  home.homeDirectory = "/Users/${user}";
  home.stateVersion = "24.11";

  programs.home-manager.enable = true;
}

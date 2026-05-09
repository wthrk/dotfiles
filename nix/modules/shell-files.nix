{ root, ... }:
{
  xdg.enable = true;

  xdg.configFile."zsh" = {
    source = "${root}/config/zsh";
    force = true;
  };

  xdg.configFile."nvim" = {
    source = "${root}/config/nvim";
    force = true;
  };
}

{ ... }:
{
  xdg.enable = true;

  xdg.configFile."zsh" = {
    source = ../config/zsh;
    force = true;
  };

  xdg.configFile."nvim" = {
    source = ../config/nvim;
    force = true;
  };
}

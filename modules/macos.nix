{ pkgs, ... }:
{
  environment.systemPackages = with pkgs; [
    mas
  ];

  fonts.packages = with pkgs; [
    noto-fonts-color-emoji
    nerd-fonts.zed-mono
  ];
}

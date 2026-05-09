# macOS ホスト全体へ入れるパッケージとフォント。
#
# Home Manager のユーザーパッケージではなく、`environment.systemPackages` と `fonts.packages` に
# 入れる必要があるものだけを扱う。
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

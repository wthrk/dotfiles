# nix-darwin の `system.defaults` で固定する macOS の既定値。
#
# Dock、Finder、キーリピートなど、ユーザー操作で変わりやすいがホスト全体で揃えたい値だけを
# rebuild ごとに同じ状態へ戻す。
{
  system.defaults = {
    dock.autohide = true;
    finder.AppleShowAllExtensions = true;
    NSGlobalDomain = {
      AppleShowAllExtensions = true;
      InitialKeyRepeat = 15;
      KeyRepeat = 2;
    };
  };
}

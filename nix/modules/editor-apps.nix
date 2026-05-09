{ lib, ... }:
{
  # Mutable-by-default policy: editor runtime/auth state is unmanaged.
  home.activation.editorPolicy = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    mkdir -p "$HOME/.config/zed"
  '';
}

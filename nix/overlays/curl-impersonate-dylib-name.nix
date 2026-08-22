# `curl-impersonate` の dylib へ絶対 store パスの install name を焼き直す overlay。
#
# 上流が残す `@rpath/...` の install name では、これを link する `python3Packages.curl-cffi` が
# LC_RPATH を持たず dlopen できない。依存する `yt-dlp` ごと darwin でビルドできなくなる。
# 同じ修正は nixpkgs master の 9da1a5ec6c87 にあり、追従先の `nixpkgs-unstable` が取り込んだら
# この file と `flake.nix` の配線を削除する。
final: prev: {
  curl-impersonate = prev.curl-impersonate.overrideAttrs (old: {
    nativeBuildInputs =
      (old.nativeBuildInputs or [ ])
      ++ prev.lib.optionals prev.stdenv.hostPlatform.isDarwin [ final.fixDarwinDylibNames ];
  });
}

# sudo の PAM auth chain へ差し込む `pam_touchid_session_guard.so` をビルドする。
#
# `openpam` は header だけを使う。Apple SDK は PAM の header も libpam の stub も持たないため、
# `security/pam_appl.h` の供給元がこれしかない。macOS の libpam は同じ OpenPAM 実装である。
#
# link はしない。`-undefined dynamic_lookup` で pam_* シンボルを解決先未定のまま残す。PAM module を
# dlopen するのは sudo が読み込んだ system の `/usr/lib/libpam.2.dylib` であり、`pam_handle_t` を作るのも
# そちらである。自前で libpam を link すると同じ process に 2 つ目の libpam が載り、片方が作った handle を
# もう片方の `pam_set_data` が書き換えることになる。解決を実行時の host process へ委ねればそれは起きない。
{
  lib,
  stdenv,
  openpam,
}:

stdenv.mkDerivation {
  pname = "pam-touchid-session-guard";
  version = "1.0.0";

  src = ./.;

  buildInputs = [ openpam ];

  buildPhase = ''
    runHook preBuild
    $CC -Wall -Wextra -Werror -O2 -bundle -undefined dynamic_lookup \
      -o pam_touchid_session_guard.so pam_touchid_session_guard.c
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    install -D -m 555 pam_touchid_session_guard.so \
      "$out/lib/pam/pam_touchid_session_guard.so"
    runHook postInstall
  '';

  meta = {
    description = "PAM module that skips Touch ID for sudo while another user holds a console session";
    platforms = lib.platforms.darwin;
  };
}

//! secrets adapter 層の公開境界。
//!
//! adapter 下位 module をそのまま露出せず、entrypoint が使う runtime adapter 生成だけを提供する。

mod piv_io;
mod yubikey;

pub(crate) fn real_secrets_boundary() -> piv_io::RealSecretsBoundary {
    piv_io::RealSecretsBoundary::default()
}

//! `dotfiles secrets` の adapter 層公開面。
//!
//! entrypoint が起動する runtime 境界実装 module を内包し、port 実装以外は外部公開しない。

mod piv_io;
mod yubikey;

pub(super) use piv_io::RealSecretsBoundary;

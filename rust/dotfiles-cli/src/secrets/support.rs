//! `dotfiles secrets` で再利用する utility 群。
//!
//! 端末 I/O、process / memory 保護、暗号 primitive 補助をここに置く。

pub(crate) mod aead;
mod oaep;
pub(crate) mod protection;
pub(crate) mod terminal;

pub(crate) use oaep::write_oaep_unpadded_sha256;

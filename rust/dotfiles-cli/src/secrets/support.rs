//! `dotfiles secrets` で再利用する utility 群。
//!
//! process / memory 保護、暗号 primitive 補助をここに置く。

pub(crate) mod aead;
mod oaep;
pub(crate) mod protection;
pub(crate) mod version;

pub(crate) use oaep::write_oaep_unpadded_sha256;

//! `dotfiles secrets` で再利用する utility 群。
//!
//! process / memory 保護と暗号 primitive 補助をここに置く。

pub(crate) mod aead;
pub(crate) mod blob_crypto;
mod oaep;
pub(crate) mod protection;

pub(crate) use oaep::write_oaep_unpadded_sha256;

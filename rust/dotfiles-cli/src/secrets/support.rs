//! `dotfiles secrets` で再利用する utility 群。
//!
//! process / memory 保護、暗号 primitive 補助など機能中立な部品だけをここに置く。

pub(crate) mod aead;
pub(crate) mod blob_crypto;
mod oaep;
pub(crate) mod protection;

pub(crate) use oaep::write_oaep_unpadded_sha256;

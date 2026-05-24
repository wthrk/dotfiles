//! `dotfiles secrets` で再利用する技術中立 utility 群。
//!
//! process / memory 保護と暗号 primitive 補助を提供し、terminal I/O は adapter 層へ分離する。

pub(crate) mod aead;
mod oaep;
pub(crate) mod protection;

pub(crate) use oaep::write_oaep_unpadded_sha256;

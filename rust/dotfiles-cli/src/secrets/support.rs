//! 秘密値処理で再利用する product-neutral utility 群。
//!
//! process / memory 保護、暗号 primitive 補助をここに置く。

pub(crate) mod aead;
pub(crate) mod process_io;
pub(crate) mod protection;

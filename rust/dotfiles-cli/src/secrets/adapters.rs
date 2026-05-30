//! `dotfiles secrets` adapter 層の module 境界。
//!
//! adapter 実装は port grouping に合わせ、`yubikey`、`bw`、`io` へ分割する。この root は
//! module 宣言だけを持ち、port 実装型の公開面は各責務別 module に閉じる。

pub(crate) mod bw;
pub(crate) mod io;
pub(crate) mod yubikey;

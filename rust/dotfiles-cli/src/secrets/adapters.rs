//! secrets adapter 層の責務別 module tree。
//!
//! 各 child module が特定の port 契約と外部技術の翻訳を所有する。concrete adapter 型は
//! 責務別 module path から参照し、adapter root では convenience re-export を作らない。

pub(crate) mod bw;
pub(crate) mod io;
pub(crate) mod yubikey;

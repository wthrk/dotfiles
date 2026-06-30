//! `dotfiles secrets` application 層が外部境界へ要求する port 契約の module 境界。
//!
//! port は backend capability ごとの submodule へ分ける。caller は capability module を
//! 直接参照し、root は契約群の配置境界だけを宣言する。

pub(crate) mod bw;
pub(crate) mod git;
pub(crate) mod gpg;
pub(crate) mod io;
pub(crate) mod yubikey;

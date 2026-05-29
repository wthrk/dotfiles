//! `dotfiles secrets` application 層が外部境界へ要求する port 契約。
//!
//! backend ごとの capability module へ分け、application から見える要求先を明確にする。
//! port root は module tree だけを定義し、capability contract は責務別 child module に置く。

pub(crate) mod bw;
pub(crate) mod io;
pub(crate) mod yubikey;

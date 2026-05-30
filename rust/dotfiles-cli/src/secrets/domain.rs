//! `dotfiles secrets` の domain 層。
//!
//! PIV object に保存する値、BWS lookup の固定規則、enrollment / verification の結果意味、
//! storage の状態遷移、wire format を定義する。port 契約は `ports/` 側へ置き、
//! 端末 I/O、process 保護、実機 YubiKey discovery は外側の責務とする。

pub(crate) mod bws;
pub(crate) mod commands;
pub(crate) mod enrollment;
pub mod manifest;
pub mod piv;
pub mod storage;
pub(crate) mod verification;
pub(crate) mod wire;

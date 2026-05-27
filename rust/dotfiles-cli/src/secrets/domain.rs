//! `dotfiles secrets` の domain 層。
//!
//! PIV object に保存する値、wire format、device port、保存規則を定義する。
//! 端末 I/O、process 保護、実機 YubiKey discovery は外側の責務とする。

pub mod manifest;
pub mod material;
pub mod piv;
pub mod values;
pub(crate) mod wire;

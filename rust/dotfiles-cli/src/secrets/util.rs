//! `dotfiles secrets` の端末操作と暗号 primitive 補助。
//!
//! ここに置く処理は command option、storage model、YubiKey discovery を受け取らない。

pub(crate) mod oaep;
pub(crate) mod protection;
pub(crate) mod terminal;

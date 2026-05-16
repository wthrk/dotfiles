//! `dotfiles secrets` の端末操作と暗号 primitive 補助。
//!
//! command option、storage model、YubiKey discovery を持たない汎用部品を置く。

pub(crate) mod oaep;
pub(crate) mod protection;
pub(crate) mod terminal;

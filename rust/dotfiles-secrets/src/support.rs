//! 秘密値処理で再利用する utility と protection backend 境界。
//!
//! process / memory 保護、暗号 primitive 補助、secret を扱う外部処理の保護境界をここに置く。

#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod aead;
pub(crate) mod clock;
pub(crate) mod process_io;
pub(crate) mod protection;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod ssh_agent_socket;

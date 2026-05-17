//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey や test stub など外部境界の都合はここに閉じ、application と domain へは port の
//! contract だけを見せる。

#[cfg(feature = "secrets-test-stub")]
mod test_stub;
mod yubikey;

#[cfg(feature = "secrets-test-stub")]
pub(crate) use test_stub::TestSecretsBoundary;
pub(crate) use yubikey::{
    SPARE_SERIAL_NONINTERACTIVE_ERROR, YubikeySecretDevice, open_device, open_spare_device,
};

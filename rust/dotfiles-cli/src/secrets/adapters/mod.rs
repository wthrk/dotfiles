//! secrets adapter 層の公開境界。

pub(crate) mod bw;
pub(crate) mod io;
#[cfg(feature = "secrets-internal-test-stub")]
// test-only backend stub module 宣言。`secrets-internal-test-stub` feature 有効時だけ compile される。
// production build には含めない。
pub(crate) mod stub;
pub(crate) mod yubikey;

pub(crate) use bw::BwsClientAdapter;
pub(crate) use io::{JsonReportAdapter, ProcessIoAdapter};
pub(crate) use yubikey::{DeviceSelectionAdapter, StorageAdapter};

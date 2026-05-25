//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。

mod backend;
mod device_prompt;
mod enrollment_json;
pub(super) mod input;
mod prompt;
pub(crate) mod real_boundary;
pub(crate) mod stdin;
mod stdout;
pub(super) mod terminal;
mod yubikey;

use crate::{secrets::support::protection::InterruptGuard, Result};

pub(crate) use backend::DeviceBackend;

/// 通常 build で application が扱う YubiKey device adapter。
pub(crate) type YubikeySecretDevice = yubikey::YubikeySecretDevice;

/// backend に対応する通常操作対象 device を開く。
///
/// 非対話時の serial 必須条件は実機 adapter の error contract にする。
pub(crate) fn open_device(
    backend: &mut DeviceBackend,
    serial: Option<u32>,
) -> Result<YubikeySecretDevice> {
    let io = device_prompt::yubikey_interaction();
    match backend {
        DeviceBackend::Real => yubikey::open_device(serial, &io),
    }
}

/// backend に対応する spare 登録対象 device を開く。
///
/// 実機 adapter では spare 待機の interrupt policy を適用する。
pub(crate) fn open_spare_device(
    backend: &mut DeviceBackend,
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    let io = device_prompt::yubikey_interaction();
    match backend {
        DeviceBackend::Real => {
            yubikey::open_spare_device(spare_serial, primary_serial, interrupt, &io)
        }
    }
}

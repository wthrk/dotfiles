//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。

mod backend;
mod device_prompt;
mod enrollment_json;
mod input;
mod prompt;
mod real_boundary;
mod stdin;
mod stdout;
mod terminal;
#[cfg(feature = "secrets-test-stub")]
mod test_stub;
mod yubikey;

#[cfg(feature = "secrets-test-stub")]
use crate::secrets::{domain::PivObjectId, ports::SecretDevice};
use crate::{secrets::support::protection::InterruptGuard, Result};

use backend::DeviceBackend;

#[cfg(feature = "secrets-test-stub")]
/// 実機 YubiKey と device stub を同じ `SecretDevice` port として扱う adapter。
///
/// `secrets-test-stub` feature でだけ enum になり、application の use case は variant を見ない。
pub(super) enum YubikeySecretDevice {
    /// 実機 YubiKey の PIV device adapter。
    Real(yubikey::YubikeySecretDevice),
    /// CLI 統合テスト用の in-memory PIV device adapter。
    TestStub(test_stub::TestDevice),
}

#[cfg(not(feature = "secrets-test-stub"))]
/// 通常 build で application が扱う YubiKey device adapter。
type YubikeySecretDevice = yubikey::YubikeySecretDevice;

#[cfg(feature = "secrets-test-stub")]
impl SecretDevice for YubikeySecretDevice {
    fn serial(&self) -> u32 {
        match self {
            Self::Real(device) => device.serial(),
            Self::TestStub(device) => device.serial(),
        }
    }

    fn key_exists(&mut self) -> Result<bool> {
        match self {
            Self::Real(device) => device.key_exists(),
            Self::TestStub(device) => device.key_exists(),
        }
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_key_generation_preconditions(),
            Self::TestStub(device) => device.check_key_generation_preconditions(),
        }
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_management_auth_preconditions(),
            Self::TestStub(device) => device.check_management_auth_preconditions(),
        }
    }

    fn generate_key(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.generate_key(),
            Self::TestStub(device) => device.generate_key(),
        }
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Real(device) => device.read_object(object_id),
            Self::TestStub(device) => device.read_object(object_id),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        match self {
            Self::Real(device) => device.write_object(object_id, value),
            Self::TestStub(device) => device.write_object(object_id, value),
        }
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Real(device) => device.wrap_key(key),
            Self::TestStub(device) => device.wrap_key(key),
        }
    }

    fn verify_pin(&mut self, pin: &[u8]) -> Result<()> {
        match self {
            Self::Real(device) => device.verify_pin(pin),
            Self::TestStub(device) => device.verify_pin(pin),
        }
    }

    fn requires_pin_input(&self) -> bool {
        match self {
            Self::Real(device) => device.requires_pin_input(),
            Self::TestStub(device) => device.requires_pin_input(),
        }
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        match self {
            Self::Real(device) => device.unwrap_key(wrapped_key),
            Self::TestStub(device) => device.unwrap_key(wrapped_key),
        }
    }
}

/// 実プロセス用の `SecretsBoundary` 実装を構築して返す。
///
/// backend の選択ロジックは adapter 層に閉じ、呼び出し元は境界型だけを受け取る。
pub(super) fn build_real_boundary(test_stub: bool) -> Result<impl crate::secrets::ports::SecretsBoundary> {
    let backend = DeviceBackend::from_test_flag(test_stub)?;
    Ok(real_boundary::RealSecretsBoundary::new(backend))
}

/// backend に対応する通常操作対象 device を開く。
///
/// 非対話時の serial 必須条件は実機 adapter の error contract にする。
pub(super) fn open_device(
    backend: &mut DeviceBackend,
    serial: Option<u32>,
) -> Result<YubikeySecretDevice> {
    let io = device_prompt::yubikey_interaction();
    match backend {
        #[cfg(feature = "secrets-test-stub")]
        DeviceBackend::TestStub(factory) => factory
            .open_device(serial)
            .map(YubikeySecretDevice::TestStub),
        DeviceBackend::Real => {
            #[cfg(feature = "secrets-test-stub")]
            {
                yubikey::open_device(serial, &io).map(YubikeySecretDevice::Real)
            }
            #[cfg(not(feature = "secrets-test-stub"))]
            {
                yubikey::open_device(serial, &io)
            }
        }
    }
}

/// backend に対応する spare 登録対象 device を開く。
///
/// 実機 adapter では spare 待機の interrupt policy を適用する。
pub(super) fn open_spare_device(
    backend: &mut DeviceBackend,
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    let io = device_prompt::yubikey_interaction();
    match backend {
        #[cfg(feature = "secrets-test-stub")]
        DeviceBackend::TestStub(factory) => factory
            .open_spare_device(spare_serial, primary_serial)
            .map(YubikeySecretDevice::TestStub),
        DeviceBackend::Real => {
            #[cfg(feature = "secrets-test-stub")]
            {
                yubikey::open_spare_device(spare_serial, primary_serial, interrupt, &io)
                    .map(YubikeySecretDevice::Real)
            }
            #[cfg(not(feature = "secrets-test-stub"))]
            {
                yubikey::open_spare_device(spare_serial, primary_serial, interrupt, &io)
            }
        }
    }
}

//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。

mod boundary;
mod input;
mod storage_service;
#[cfg(feature = "secrets-test-stub")]
mod test_stub;
mod terminal;
mod yubikey;

#[cfg(feature = "secrets-test-stub")]
use crate::secrets::domain::PivObjectId;
use crate::{secrets::support::protection::InterruptGuard, Result};
use crate::secrets::{
    domain::SecretName,
    ports::SecretDevice,
    support::protection::{ProtectedSecret, SecretSession},
};

pub(super) use boundary::RealSecretsBoundary;

#[cfg(feature = "secrets-test-stub")]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// application はこの値を保持するだけで、実機か stub かに応じた別 use case を持たない。
pub(super) enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
    /// CLI 統合テスト用の in-memory device adapter を使う実行。
    TestStub(test_stub::TestDeviceFactory),
}

#[cfg(not(feature = "secrets-test-stub"))]
#[derive(Clone, Copy)]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// 通常 build では実機 adapter だけを持ち、stub 用の実行経路を含めない。
pub(super) enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
}

impl DeviceBackend {
    #[cfg(feature = "secrets-test-stub")]
    /// CLI option から device adapter の選択状態を構築する。
    ///
    /// `secrets-test-stub` feature 有効時だけ hidden test flag を解釈し、stub の初期状態は
    /// integration test contract の環境変数から読む。
    pub(super) fn from_test_flag(enabled: bool) -> Result<Self> {
        if enabled {
            return Ok(Self::TestStub(test_stub::TestDeviceFactory::from_env()?));
        }
        Ok(Self::Real)
    }

    #[cfg(not(feature = "secrets-test-stub"))]
    /// 通常 build で実機 adapter の選択状態を構築する。
    ///
    /// stub 用 flag は clap 定義に存在しないため、この build では常に実機 adapter を選ぶ。
    pub(super) fn from_test_flag(_enabled: bool) -> Result<Self> {
        Ok(Self::Real)
    }
}

#[cfg(feature = "secrets-test-stub")]
/// 実機 YubiKey と device stub を同じ `SecretDevice` port として扱う adapter。
///
/// `secrets-test-stub` feature でだけ enum になり、application の use case は variant を見ない。
pub(in crate::secrets) enum YubikeySecretDevice {
    /// 実機 YubiKey の PIV device adapter。
    Real(yubikey::YubikeySecretDevice),
    /// CLI 統合テスト用の in-memory PIV device adapter。
    TestStub(test_stub::TestDevice),
}

#[cfg(not(feature = "secrets-test-stub"))]
/// 通常 build で application が扱う YubiKey device adapter。
pub(in crate::secrets) type YubikeySecretDevice = yubikey::YubikeySecretDevice;

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

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Real(device) => device.unwrap_key(wrapped_key),
            Self::TestStub(device) => device.unwrap_key(wrapped_key),
        }
    }
}

/// backend に対応する通常操作対象 device を開く。
///
/// 非対話時の serial 必須条件は実機 adapter と stub adapter の両方で同じ error contract にする。
fn open_device(
    backend: &mut DeviceBackend,
    serial: Option<u32>,
) -> Result<YubikeySecretDevice> {
    match backend {
        #[cfg(feature = "secrets-test-stub")]
        DeviceBackend::TestStub(factory) => factory
            .open_device(serial)
            .map(YubikeySecretDevice::TestStub),
        DeviceBackend::Real => {
            #[cfg(feature = "secrets-test-stub")]
            {
                yubikey::open_device(serial).map(YubikeySecretDevice::Real)
            }
            #[cfg(not(feature = "secrets-test-stub"))]
            {
                yubikey::open_device(serial)
            }
        }
    }
}

/// backend に対応する spare 登録対象 device を開く。
///
/// 実機 adapter では spare 待機の interrupt policy を適用し、stub adapter では同じ serial
/// 制約を in-memory device に対して適用する。
fn open_spare_device(
    backend: &mut DeviceBackend,
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    match backend {
        #[cfg(feature = "secrets-test-stub")]
        DeviceBackend::TestStub(factory) => factory
            .open_spare_device(spare_serial, primary_serial)
            .map(YubikeySecretDevice::TestStub),
        DeviceBackend::Real => {
            #[cfg(feature = "secrets-test-stub")]
            {
                yubikey::open_spare_device(spare_serial, primary_serial, interrupt)
                    .map(YubikeySecretDevice::Real)
            }
            #[cfg(not(feature = "secrets-test-stub"))]
            {
                yubikey::open_spare_device(spare_serial, primary_serial, interrupt)
            }
        }
    }
}

pub(super) fn setup_storage<D: SecretDevice>(device: &mut D) -> Result<()> {
    storage_service::setup(device)
}

pub(super) fn check_setup_preconditions<D: SecretDevice>(device: &mut D) -> Result<()> {
    storage_service::check_setup_preconditions(device)
}

pub(super) fn put_secret<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    secret: &[u8],
    force: bool,
    session: &SecretSession,
) -> Result<()> {
    storage_service::put(device, name, secret, force, session)
}

pub(super) fn check_put_preconditions<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    force: bool,
) -> Result<()> {
    storage_service::check_put_preconditions(device, name, force)
}

pub(super) fn get_secret_protected<'session, D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    session: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    storage_service::get_protected(device, name, session)
}

pub(super) fn replace_bws_token<D: SecretDevice>(
    device: &mut D,
    token: &[u8],
    session: &SecretSession,
) -> Result<()> {
    storage_service::replace_bws_token(device, token, session)
}

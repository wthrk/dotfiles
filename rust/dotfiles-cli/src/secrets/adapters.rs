//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。

mod boundary;
mod input;
mod storage_service;
mod terminal;
mod yubikey;

use crate::{secrets::support::protection::InterruptGuard, Result};
use crate::secrets::{
    domain::SecretName,
    ports::SecretDevice,
    support::protection::{ProtectedSecret, SecretSession},
};

pub(super) use boundary::RealSecretsBoundary;

#[derive(Clone, Copy)]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// 実機 adapter だけを持ち、test double は production 経路へ含めない。
pub(super) enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
}

impl DeviceBackend {
    /// 実機 adapter の選択状態を構築する。
    pub(super) fn new() -> Self {
        Self::Real
    }
}

/// 通常 build で application が扱う YubiKey device adapter。
pub(in crate::secrets) type YubikeySecretDevice = yubikey::YubikeySecretDevice;

/// backend に対応する通常操作対象 device を開く。
///
/// 非対話時の serial 必須条件は実機 adapter と stub adapter の両方で同じ error contract にする。
fn open_device(
    backend: &mut DeviceBackend,
    serial: Option<u32>,
) -> Result<YubikeySecretDevice> {
    match backend {
        DeviceBackend::Real => yubikey::open_device(serial),
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
        DeviceBackend::Real => yubikey::open_spare_device(spare_serial, primary_serial, interrupt),
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

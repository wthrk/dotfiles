//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。

#[cfg(feature = "secrets-test-stub")]
mod test_stub;
mod yubikey;

use anyhow::Context;
#[cfg(feature = "secrets-test-stub")]
use std::io::Write;
use std::{io, time::Instant};

#[cfg(feature = "secrets-test-stub")]
use crate::secrets::domain::{PivObjectId, SecretDevice};
use crate::secrets::support::terminal::{read_terminal_line_until, wait_for_enter};
use crate::{Result, secrets::support::protection::InterruptGuard};

#[cfg(feature = "secrets-test-stub")]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// application はこの値を保持するだけで、実機か stub かに応じた別 use case を持たない。
pub(crate) enum DeviceBackend {
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
pub(crate) enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
}

impl DeviceBackend {
    #[cfg(feature = "secrets-test-stub")]
    /// CLI option から device adapter の選択状態を構築する。
    ///
    /// `secrets-test-stub` feature 有効時だけ hidden test flag を解釈し、stub の初期状態は
    /// integration test contract の環境変数から読む。
    pub(crate) fn from_test_flag(enabled: bool) -> Result<Self> {
        if enabled {
            return Ok(Self::TestStub(test_stub::TestDeviceFactory::from_env()?));
        }
        Ok(Self::Real)
    }

    #[cfg(not(feature = "secrets-test-stub"))]
    /// 通常 build で実機 adapter の選択状態を構築する。
    ///
    /// stub 用 flag は clap 定義に存在しないため、この build では常に実機 adapter を選ぶ。
    pub(crate) fn from_test_flag(_enabled: bool) -> Result<Self> {
        Ok(Self::Real)
    }
}

#[cfg(feature = "secrets-test-stub")]
/// 実機 YubiKey と device stub を同じ `SecretDevice` port として扱う adapter。
///
/// `secrets-test-stub` feature でだけ enum になり、application の use case は variant を見ない。
pub(crate) enum YubikeySecretDevice {
    /// 実機 YubiKey の PIV device adapter。
    Real(yubikey::YubikeySecretDevice),
    /// CLI 統合テスト用の in-memory PIV device adapter。
    TestStub(test_stub::TestDevice),
}

#[cfg(not(feature = "secrets-test-stub"))]
/// 通常 build で application が扱う YubiKey device adapter。
pub(crate) type YubikeySecretDevice = yubikey::YubikeySecretDevice;

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

    fn write_unwrapped_key(&mut self, wrapped_key: &[u8], output: &mut impl Write) -> Result<()> {
        match self {
            Self::Real(device) => device.write_unwrapped_key(wrapped_key, output),
            Self::TestStub(device) => device.write_unwrapped_key(wrapped_key, output),
        }
    }
}

/// backend に対応する通常操作対象 device を開く。
///
/// 非対話時の serial 必須条件は実機 adapter と stub adapter の両方で同じ error contract にする。
pub(crate) fn open_device(
    backend: &mut DeviceBackend,
    serial: Option<u32>,
) -> Result<YubikeySecretDevice> {
    let io = yubikey_interaction();
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
/// 実機 adapter では spare 待機の interrupt policy を適用し、stub adapter では同じ serial
/// 制約を in-memory device に対して適用する。
pub(crate) fn open_spare_device(
    backend: &mut DeviceBackend,
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    let io = yubikey_interaction();
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

/// 実機 YubiKey adapter の対話 I/O 境界を組み立てる。
///
/// reader 選択と spare 差し替え待機だけをここへ集約し、`yubikey` module は device 操作へ専念させる。
fn yubikey_interaction<'a>() -> yubikey::YubikeyInteraction<'a> {
    yubikey::YubikeyInteraction {
        select_candidate: &select_yubikey_candidate,
        wait_for_spare_replacement: &wait_for_spare_replacement,
    }
}

/// 複数の YubiKey 候補を表示し、利用者が選んだ index を返す。
///
/// 非対話実行の判定は caller 側で完了してから呼ばれるため、この関数は候補表示と番号入力だけを扱う。
fn select_yubikey_candidate(
    candidates: &[yubikey::YubikeySelectionCandidate<'_>],
    timed_input: Option<(Instant, &InterruptGuard)>,
) -> Result<usize> {
    eprintln!("Select YubiKey:");
    for (index, candidate) in candidates.iter().enumerate() {
        eprintln!(
            "{}: serial {} ({})",
            index + 1,
            candidate.serial,
            candidate.reader
        );
    }
    eprint!("number: ");
    std::io::Write::flush(&mut io::stderr())?;

    let input = if let Some((deadline, interrupt)) = timed_input {
        read_terminal_line_until(deadline, interrupt, "timed out waiting for spare YubiKey")?
    } else {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        input
    };
    let selected = input
        .trim()
        .parse::<usize>()
        .map_err(anyhow::Error::from)
        .context("invalid selection")?;
    if selected == 0 || selected > candidates.len() {
        anyhow::bail!("selected YubiKey is out of range");
    }
    Ok(selected - 1)
}

/// primary と同じ device が選ばれた後、spare への差し替え完了を Enter で待つ。
///
/// 待機は spare 登録の deadline と interrupt policy に従う。
fn wait_for_spare_replacement(deadline: Instant, interrupt: &InterruptGuard) -> Result<()> {
    eprintln!("The selected YubiKey is the primary; replace it with the spare.");
    eprintln!("Insert the spare YubiKey, then press Enter.");
    wait_for_enter(
        deadline,
        interrupt,
        "pass --spare-serial in non-interactive use",
        "timed out waiting for spare YubiKey",
    )
}

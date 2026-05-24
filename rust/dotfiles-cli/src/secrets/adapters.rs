//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。

pub(crate) mod boundary;
pub(super) mod input;
pub(super) mod terminal;
mod yubikey;

use anyhow::Context;
use std::{io, time::Instant};

use crate::secrets::adapters::terminal::{
    read_terminal_line_interruptible, read_terminal_line_until, wait_for_enter,
};
use crate::{secrets::support::protection::InterruptGuard, Result};

#[derive(Clone, Copy)]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// 実行時に利用する device backend は実機 adapter のみに固定する。
pub(crate) enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
}

impl DeviceBackend {
    /// 通常実行で使う device backend を構築する。
    pub(crate) fn real() -> Self {
        Self::Real
    }
}

/// 通常 build で application が扱う YubiKey device adapter。
pub(crate) type YubikeySecretDevice = yubikey::YubikeySecretDevice;

/// backend に対応する通常操作対象 device を開く。
///
/// 非対話時の serial 必須条件は実機 adapter と stub adapter の両方で同じ error contract にする。
pub(crate) fn open_device(
    backend: &mut DeviceBackend,
    serial: Option<u32>,
) -> Result<YubikeySecretDevice> {
    let io = yubikey_interaction();
    match backend {
        DeviceBackend::Real => yubikey::open_device(serial, &io),
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
        DeviceBackend::Real => yubikey::open_spare_device(spare_serial, primary_serial, interrupt, &io),
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
        let interrupt = InterruptGuard::install()
            .context("failed to install interrupt handler for YubiKey selection")?;
        read_terminal_line_interruptible(&interrupt)?
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
        "cannot wait for spare YubiKey replacement without a controlling terminal",
        "timed out waiting for spare YubiKey",
    )
}

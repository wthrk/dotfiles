//! YubiKey device 選択 prompt adapter。
//!
//! 複数候補の表示と入力受け付け、spare 差し替え待機を担う。

use std::{io, time::Instant};

use anyhow::Context;

use super::{terminal, yubikey};
use crate::{secrets::support::protection::InterruptGuard, Result};

/// 実機 YubiKey adapter の対話 I/O 境界を組み立てる。
///
/// reader 選択と spare 差し替え待機だけをここへ集約し、`yubikey` module は device 操作へ専念させる。
pub(super) fn yubikey_interaction<'a>() -> yubikey::YubikeyInteraction<'a> {
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
        terminal::read_terminal_line_until(
            deadline,
            interrupt,
            "timed out waiting for spare YubiKey",
        )?
    } else {
        let interrupt = InterruptGuard::install()
            .context("failed to install interrupt handler for YubiKey selection")?;
        terminal::read_terminal_line_interruptible(&interrupt)?
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
    terminal::wait_for_enter(
        deadline,
        interrupt,
        "cannot wait for spare YubiKey replacement without a controlling terminal",
        "timed out waiting for spare YubiKey",
    )
}

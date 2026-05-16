//! 対話 prompt、raw mode 入力、stdout 書き込みを扱う端末境界。
//!
//! secret の保存形式や JSON schema は扱わない。spare 差し替え待ちだけは期限と
//! `InterruptGuard` を受け取り、入力待ちを command の中断境界に合わせる。

use std::{
    io::{self, BufRead, IsTerminal, Read, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use zeroize::Zeroizing;

use crate::Result;

use super::protection::InterruptGuard;

/// 非対話実行で spare serial なしに差し替え prompt へ進まないための error。
pub(crate) const SPARE_SERIAL_NONINTERACTIVE_ERROR: &str =
    "pass --spare-serial in non-interactive use";
/// spare 差し替え待ちの期限切れを command error として返す sentinel。
pub(crate) const SPARE_WAIT_TIMEOUT_ERROR: &str = "timed out waiting for spare YubiKey";

/// YubiKey 選択 prompt に表示する reader 名と serial。
pub(crate) struct YubikeySelectionCandidate<'a> {
    pub(crate) reader: &'a str,
    pub(crate) serial: u32,
}

/// secret prompt や YubiKey 選択 prompt に入れる入力元か判定する。
pub(crate) fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

/// 低水準 `get` で平文 secret を直接画面へ出さないための出力先判定。
pub(crate) fn stdout_is_terminal() -> bool {
    io::stdout().is_terminal()
}

/// hidden prompt から得た文字列は byte 化した直後から zeroize 対象にする。
pub(crate) fn read_hidden_bytes(prompt: &str) -> Result<Zeroizing<Vec<u8>>> {
    let value = rpassword::prompt_password(prompt)?;
    Ok(Zeroizing::new(value.into_bytes()))
}

/// 保存対象 secret の hidden prompt は読み込み直後に byte 上限を検証する。
pub(crate) fn read_hidden_bytes_with_limit(
    prompt: &str,
    limit: usize,
    too_large_error: &'static str,
) -> Result<Zeroizing<Vec<u8>>> {
    let value = read_hidden_bytes(prompt)?;
    if value.len() > limit {
        bail!(too_large_error);
    }
    Ok(value)
}

/// 表示 prompt の行入力は `limit + 1` byte だけ読み、上限超過を即時に検出する。
pub(crate) fn read_visible_line_bytes(
    prompt: &str,
    limit: usize,
    too_large_error: &'static str,
) -> Result<Zeroizing<Vec<u8>>> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut input = Zeroizing::new(Vec::with_capacity(limit.min(4096)));
    let len = io::stdin()
        .lock()
        .take((limit + 1) as u64)
        .read_until(b'\n', &mut input)?;
    if len > limit {
        bail!(too_large_error);
    }
    trim_one_trailing_newline(&mut input);
    Ok(input)
}

/// 非対話実行では追加更新の確認 prompt に入らない。
pub(crate) fn prompt_yes_no(prompt: &str) -> Result<bool> {
    if !stdin_is_terminal() {
        return Ok(false);
    }

    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// spare 差し替え待ちは期限切れまたは中断を Enter より優先して返す。
pub(crate) fn wait_for_enter(deadline: Instant, interrupt: &InterruptGuard) -> Result<()> {
    if !stdin_is_terminal() {
        bail!(SPARE_SERIAL_NONINTERACTIVE_ERROR);
    }
    read_terminal_line_until(deadline, interrupt).map(|_| ())
}

/// primary が選ばれた場合だけ、spare 挿入を Enter 入力で同期する。
pub(crate) fn wait_for_spare_replacement(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<()> {
    eprintln!("The selected YubiKey is the primary; replace it with the spare.");
    eprintln!("Insert the spare YubiKey, then press Enter.");
    wait_for_enter(deadline, interrupt)
}

/// 複数候補の選択 prompt は対話実行に限定し、非対話では serial 指定を要求する。
pub(crate) fn select_yubikey_candidate(
    candidates: &[YubikeySelectionCandidate<'_>],
    timed_input: Option<(Instant, &InterruptGuard)>,
) -> Result<usize> {
    if !stdin_is_terminal() {
        bail!("multiple YubiKeys detected; pass a serial option in non-interactive use");
    }

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
    io::stderr().flush()?;

    let input = if let Some((deadline, interrupt)) = timed_input {
        read_terminal_line_until(deadline, interrupt)?
    } else {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        input
    };
    let selected = input.trim().parse::<usize>().context("invalid selection")?;
    if selected == 0 || selected > candidates.len() {
        bail!("selected YubiKey is out of range");
    }

    Ok(selected - 1)
}

/// caller が TTY 拒否を済ませた後、secret bytes を pipe/redirect へ書き込む。
pub(crate) fn write_all_stdout(bytes: &[u8]) -> Result<()> {
    io::stdout().lock().write_all(bytes)?;
    Ok(())
}

/// raw mode の event loop で Enter、Ctrl-C、deadline、interrupt を同じ thread で扱う。
pub(crate) fn read_terminal_line_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<String> {
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    let mut line = String::new();
    loop {
        interrupt.check_interrupted()?;
        let now = Instant::now();
        if now >= deadline {
            bail!(SPARE_WAIT_TIMEOUT_ERROR);
        }
        let timeout = Duration::from_millis(100).min(deadline.saturating_duration_since(now));

        if !event::poll(timeout)? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Enter => {
                eprintln!();
                return Ok(line);
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                bail!("interrupted while reading terminal input");
            }
            KeyCode::Char(ch) => {
                line.push(ch);
                eprint!("{ch}");
                io::stderr().flush()?;
            }
            KeyCode::Backspace => {
                if line.pop().is_some() {
                    eprint!("\u{8} \u{8}");
                    io::stderr().flush()?;
                }
            }
            _ => {}
        }
    }
}

/// prompt/stdin 由来の末尾 newline 1 個だけを保存対象から外す。
fn trim_one_trailing_newline(input: &mut Vec<u8>) {
    if input.ends_with(b"\n") {
        input.pop();
        if input.ends_with(b"\r") {
            input.pop();
        }
    }
}

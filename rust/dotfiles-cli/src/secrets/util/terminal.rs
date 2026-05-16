//! 端末の標準入出力、TTY 判定、raw mode 入力を扱う I/O adapter。
//!
//! prompt 表示、TTY 判定、deadline、interrupt policy、失敗理由を受け取り、
//! 端末入力の完了または中断を呼び出し側へ返す。

use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crate::Result;
use anyhow::{Context, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use super::protection::InterruptGuard;

/// 現在の stdin が対話入力を読める TTY かを返す。
pub(crate) fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

/// 現在の stdout が画面表示される TTY かを返す。
pub(crate) fn stdout_is_terminal() -> bool {
    io::stdout().is_terminal()
}

/// TTY では prompt を stderr へ表示し、stdin の 1 行を yes/no 応答として返す。
///
/// stdin が TTY でない場合は入力を読まずに `false` を返す。
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

/// TTY で Enter 入力を待ち、入力完了、期限切れ、中断のいずれかを返す。
///
/// stdin が TTY でない場合と deadline 超過時の error 文言は呼び出し側が指定する。
pub(crate) fn wait_for_enter(
    deadline: Instant,
    interrupt: &InterruptGuard,
    noninteractive_error: &'static str,
    timeout_error: &'static str,
) -> Result<()> {
    if !stdin_is_terminal() {
        bail!(noninteractive_error);
    }
    read_terminal_line_until(deadline, interrupt, timeout_error).map(|_| ())
}

/// byte 列を stdout へそのまま書き込む。
pub(crate) fn write_all_stdout(bytes: &[u8]) -> Result<()> {
    io::stdout().lock().write_all(bytes)?;
    Ok(())
}

/// raw mode で 1 行を読み、Enter で入力を確定してそれまでの文字列を返す。
///
/// Ctrl-C、中断 flag、deadline 超過を同じ loop で監視し、deadline 超過時の error 文言は
/// 呼び出し側が指定する。
pub(crate) fn read_terminal_line_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
    timeout_error: &'static str,
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
            bail!(timeout_error);
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

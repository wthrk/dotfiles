//! 端末の標準入出力、TTY 判定、raw mode 入力を扱う I/O adapter。
//!
//! prompt 表示、TTY 判定、deadline、interrupt policy、失敗理由を受け取り、
//! 端末入力の完了または中断を呼び出し側へ返す。

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

/// 現在の stdin が対話入力を読める TTY かを返す。
pub(crate) fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

/// 現在の stdout が画面表示される TTY かを返す。
pub(crate) fn stdout_is_terminal() -> bool {
    io::stdout().is_terminal()
}

/// prompt を表示して echo なしで 1 行を読む。
///
/// 返す byte buffer は生成直後から zeroize 対象にする。
pub(crate) fn read_hidden_bytes(prompt: &str) -> Result<Zeroizing<Vec<u8>>> {
    let value = rpassword::prompt_password(prompt)?;
    Ok(Zeroizing::new(value.into_bytes()))
}

/// echo なしで読んだ 1 行を byte buffer として返す。
///
/// 上限超過時は指定 error で失敗する。
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

/// prompt を stderr へ表示して stdin から 1 行を読む。
///
/// 戻り値は末尾改行を除いた byte buffer とする。
///
/// 上限超過時は指定 error で失敗する。
pub(crate) fn read_visible_line_bytes(
    prompt: &str,
    limit: usize,
    too_large_error: &'static str,
) -> Result<Zeroizing<Vec<u8>>> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let read_limit = limit + 3;
    let mut input = Zeroizing::new(Vec::with_capacity(read_limit.min(4096)));
    io::stdin()
        .lock()
        .take(read_limit as u64)
        .read_until(b'\n', &mut input)?;
    trim_one_trailing_newline(&mut input);
    if input.len() > limit {
        bail!(too_large_error);
    }
    Ok(input)
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

/// stdin の行入力で付く末尾の LF または CRLF を 1 行分だけ取り除く。
fn trim_one_trailing_newline(input: &mut Vec<u8>) {
    if input.ends_with(b"\n") {
        input.pop();
        if input.ends_with(b"\r") {
            input.pop();
        }
    }
}

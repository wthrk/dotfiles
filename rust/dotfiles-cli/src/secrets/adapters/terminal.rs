//! 端末の標準入出力、TTY 判定、raw mode 入力を扱う I/O adapter。
//!
//! prompt 表示、TTY 判定、deadline、interrupt policy、失敗理由を受け取り、
//! 端末入力の完了または中断を呼び出し側へ返す。

use std::{
    fs::OpenOptions,
    io::{self, IsTerminal, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::Result;
use anyhow::{Context, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use crate::secrets::support::protection::{InterruptGuard, ProtectedInputBuffer, SecretSession};

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
pub(crate) fn prompt_yes_no(prompt: &str, interrupt: &InterruptGuard) -> Result<bool> {
    if !stdin_is_terminal() {
        return Ok(false);
    }

    eprint!("{prompt}");
    io::stderr().flush()?;
    let input = read_terminal_line_interruptible(interrupt)?;
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

/// echo せずに TTY から 1 行を読み、保護済み入力 buffer へ保持する。
///
/// 入力 bytes は `SecretSession` の memory lock 範囲へ直接書き込み、Enter で確定する。
/// stdin が pipe の場合は controlling terminal を開き、secret payload 用 stdin を消費しない。
pub(crate) fn read_hidden_input(
    prompt: &str,
    limit: usize,
    limit_error: &'static str,
    session: &SecretSession,
) -> Result<ProtectedInputBuffer> {
    eprint!("{prompt}");
    io::stderr().flush()?;

    let mut reader = hidden_input_reader()?;
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    let mut input = ProtectedInputBuffer::new(limit + 1, session)?;
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte)? == 0 {
            return Ok(input);
        }

        match byte[0] {
            b'\r' | b'\n' => {
                eprintln!();
                return Ok(input);
            }
            3 => {
                bail!("interrupted while reading hidden input");
            }
            8 | 127 => {
                input.pop_byte();
            }
            value => {
                input.write_all(&[value])?;
                if input.as_slice().len() > limit {
                    bail!(limit_error);
                }
            }
        }
    }
}

/// `stdin.read_line` の EOF 契約を保ったまま、interrupt flag を監視して 1 行入力を返す。
///
/// 標準入出力には portable な非同期 cancel API がないため、読み取り自体は worker thread に任せ、
/// 呼び出し側は一定間隔で interrupt flag を確認する。
pub(crate) fn read_terminal_line_interruptible(interrupt: &InterruptGuard) -> Result<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut input = String::new();
        let result = io::stdin().read_line(&mut input).map(|_| input);
        let _ = sender.send(result);
    });

    loop {
        interrupt.check_interrupted()?;
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return Ok(result?),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("failed to read terminal input")
            }
        }
    }
}

/// hidden prompt の入力元を、stdin または controlling terminal として開く。
///
/// stdin payload と PIN / hidden prompt を併用する経路では `/dev/tty` を使う。
fn hidden_input_reader() -> Result<Box<dyn io::Read>> {
    if stdin_is_terminal() {
        return Ok(Box::new(io::stdin()));
    }

    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("failed to open controlling terminal")?;
    Ok(Box::new(tty))
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

        if let Some(completed) = read_terminal_key_event(&mut line)? {
            return Ok(completed);
        }
    }
}

fn read_terminal_key_event(line: &mut String) -> Result<Option<String>> {
    let Event::Key(key) = event::read()? else {
        return Ok(None);
    };
    if key.kind != KeyEventKind::Press {
        return Ok(None);
    }

    match key.code {
        KeyCode::Enter => {
            eprintln!();
            return Ok(Some(line.clone()));
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
    Ok(None)
}

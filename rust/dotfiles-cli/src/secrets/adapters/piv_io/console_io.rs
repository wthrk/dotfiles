//! `dotfiles secrets` の端末 I/O 境界を担う adapter 内部実装。
//!
//! prompt / stdin / stdout の扱いは support 層へ置かず、port 実装を担う adapter 側へ閉じる。

use std::{
    fs::OpenOptions,
    io::{self, IsTerminal, Read, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

use anyhow::{Context, bail};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use zeroize::Zeroizing;

use crate::{
    Result,
    secrets::support::protection::{ProtectedInputBuffer, SecretSession},
};

pub(super) const SECRET_STDOUT_TERMINAL_ERROR: &str =
    "refusing to write secret to terminal; redirect stdout to a file or pipe";

pub(super) fn read_hidden_line_bytes(
    prompt: &str,
    max_len: usize,
    too_long_error: &'static str,
) -> Result<Zeroizing<Vec<u8>>> {
    let session = SecretSession::start()?;
    let protected = read_hidden_input_line(prompt, max_len, too_long_error, &session)
        .and_then(|input| input.into_protected_secret_line(&session, max_len, too_long_error))?;
    Ok(Zeroizing::new(protected.with_secret(|b| b.to_vec())))
}

pub(super) fn read_visible_line_bytes(
    prompt: &str,
    max_len: usize,
    too_long_error: &'static str,
) -> Result<Zeroizing<Vec<u8>>> {
    let session = SecretSession::start()?;
    eprint!("{prompt}");
    io::stderr().flush()?;
    let input = read_visible_input_line(max_len, &session)?;
    let protected = input.into_protected_secret_line(&session, max_len, too_long_error)?;
    Ok(Zeroizing::new(protected.with_secret(|b| b.to_vec())))
}

pub(super) fn read_stdin_line_bytes(
    max_len: usize,
    noninteractive_error: &'static str,
    too_long_error: &'static str,
) -> Result<Zeroizing<Vec<u8>>> {
    if io::stdin().is_terminal() {
        bail!(noninteractive_error);
    }
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_line_from(std::io::stdin(), max_len, &session)?;
    let protected = input.into_protected_secret_line(&session, max_len, too_long_error)?;
    Ok(Zeroizing::new(protected.with_secret(|b| b.to_vec())))
}

pub(super) fn read_stdin_all_bytes(
    max_len: usize,
    noninteractive_error: &'static str,
    too_long_error: &'static str,
) -> Result<Zeroizing<Vec<u8>>> {
    if io::stdin().is_terminal() {
        bail!(noninteractive_error);
    }
    let session = SecretSession::start()?;
    let input =
        ProtectedInputBuffer::read_from(std::io::stdin(), max_len, too_long_error, &session)?;
    Ok(Zeroizing::new(input.as_slice().to_vec()))
}

pub(super) fn write_stdout_bytes_if_not_terminal(
    bytes: &[u8],
    terminal_error: &'static str,
) -> Result<()> {
    if io::stdout().is_terminal() {
        bail!(terminal_error);
    }
    io::stdout().lock().write_all(bytes)?;
    Ok(())
}

pub(super) fn read_prompt_line(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut reader = hidden_input_reader()?;
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte)? == 0 {
            break;
        }
        match byte[0] {
            b'\r' | b'\n' => break,
            value => out.push(value),
        }
    }
    String::from_utf8(out).context("terminal input must be UTF-8")
}

fn hidden_input_reader() -> Result<Box<dyn io::Read>> {
    if io::stdin().is_terminal() {
        return Ok(Box::new(io::stdin()));
    }

    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("failed to open controlling terminal")?;
    Ok(Box::new(tty))
}

fn read_hidden_input_line(
    prompt: &str,
    max_len: usize,
    too_long_error: &'static str,
    session: &SecretSession,
) -> Result<ProtectedInputBuffer> {
    eprint!("{prompt}");
    io::stderr().flush()?;

    let mut reader = hidden_input_reader()?;
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });

    let mut input = ProtectedInputBuffer::new(max_len + 1, session)?;
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
            3 => bail!("interrupted while reading hidden input"),
            8 | 127 => input.pop_byte(),
            value => {
                input.write_all(&[value])?;
                if input.as_slice().len() > max_len {
                    bail!(too_long_error);
                }
            }
        }
    }
}

fn read_visible_input_line(
    max_len: usize,
    session: &SecretSession,
) -> Result<ProtectedInputBuffer> {
    let read_limit = max_len + 3;
    let mut input = ProtectedInputBuffer::new(read_limit, session)?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut byte = [0u8; 1];
        loop {
            match stdin.read(&mut byte) {
                Ok(0) => {
                    let _ = sender.send(Ok(None));
                    break;
                }
                Ok(_) => {
                    let _ = sender.send(Ok(Some(byte[0])));
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(err) => {
                    let _ = sender.send(Err(err));
                    break;
                }
            }
        }
    });

    loop {
        if input.as_slice().len() >= read_limit {
            break;
        }
        session.check_interrupted()?;
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(Some(byte))) => {
                input.write_all(&[byte])?;
                if byte == b'\n' {
                    break;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(err)) => return Err(err.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("failed to read terminal input")
            }
        }
    }

    Ok(input)
}

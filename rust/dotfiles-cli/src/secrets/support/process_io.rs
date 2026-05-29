//! process 標準入出力と制御端末を扱う汎用 I/O 補助。
//!
//! この module は YubiKey や use case 名を知らず、端末 raw mode、stdin/stdout の TTY 判定、
//! byte 読み取り、保護済み入力 buffer への移送だけを担当する。

use std::io::{self, IsTerminal, Read, Write};

use anyhow::{Context, bail};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLERR, POLLHUP, POLLIN, poll, pollfd};

use crate::Result;

use super::protection::{ProtectedSecret, SecretSession, buffer::ProtectedInputBuffer};

/// 制御端末優先の reader を返し、pipe 実行時も対話入力境界を維持する。
fn stdin_or_tty_reader() -> Result<FileDescriptor> {
    if io::stdin().is_terminal() {
        FileDescriptor::dup(&io::stdin()).map_err(Into::into)
    } else {
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("failed to open controlling terminal")?;
        Ok(FileDescriptor::new(tty))
    }
}

/// 現在の stdin が terminal に接続されているかを返す。
///
/// feature 固有の判断は行わず、adapter が対話選択を許可できる process 状態だけを公開する。
pub(crate) fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

/// 制御端末から非 secret の 1 行を読み取る。
///
/// 番号選択など secret ではない入力だけに使い、secret payload は `read_hidden_line` か
/// `read_visible_line` の protected buffer 経路へ渡す。
pub(crate) fn read_control_line(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut reader = stdin_or_tty_reader()?;
    let mut line = String::new();
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte)? == 0 {
            break;
        }
        match byte[0] {
            b'\r' | b'\n' => break,
            value => line.push(char::from(value)),
        }
    }
    Ok(line)
}

/// hidden input reader の readable 状態を待つ。
fn read_hidden_byte(reader: &mut FileDescriptor, byte: &mut [u8; 1]) -> Result<usize> {
    loop {
        let mut fds = [pollfd {
            fd: reader.as_raw_file_descriptor(),
            events: POLLIN,
            revents: 0,
        }];
        let ready = poll(&mut fds, None).context("failed to poll hidden input")?;
        if ready == 0 {
            continue;
        }
        if fds[0].revents & (POLLERR | POLLHUP) != 0 && fds[0].revents & POLLIN == 0 {
            return Ok(0);
        }
        return reader.read(byte).map_err(Into::into);
    }
}

/// 非表示入力を raw mode で読み取り、入力 bytes を保護メモリのまま返す。
///
/// backspace と Ctrl-C を process I/O 境界で吸収する。
pub(crate) fn read_hidden_line(
    prompt: &str,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    let session = SecretSession::start()?;
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut reader = stdin_or_tty_reader()?;
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    let mut input = ProtectedInputBuffer::new(max_len + 1, &session)?;
    let mut byte = [0u8; 1];
    loop {
        if read_hidden_byte(&mut reader, &mut byte)? == 0 {
            break;
        }
        match byte[0] {
            b'\r' | b'\n' => {
                eprintln!();
                break;
            }
            3 => bail!("interrupted while reading hidden input"),
            8 | 127 => input.pop_byte(),
            value => {
                input.write_all(&[value])?;
                if input.as_slice().len() > max_len {
                    bail!("{too_long_message}");
                }
            }
        }
    }
    input.into_protected_secret_line(&session, max_len, too_long_message)
}

/// visible 入力を保護バッファへ直接取り込み、平文コピーを残さない。
pub(crate) fn read_visible_line(
    prompt: &str,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    let session = SecretSession::start()?;
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut reader = stdin_or_tty_reader()?;
    let input = ProtectedInputBuffer::read_line_from(&mut reader, max_len, &session)?;
    input.into_protected_secret_line(&session, max_len, too_long_message)
}

/// stdin 1 行を読み取り、末尾改行を除いた保護済み secret を返す。
pub(crate) fn read_stdin_line(
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    if io::stdin().is_terminal() {
        bail!("stdin secret input requires pipe or redirect input");
    }
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_line_from(io::stdin(), max_len, &session)?;
    input.into_protected_secret_line(&session, max_len, too_long_message)
}

/// stdin 全体を保護バッファへ読み取り、末尾改行を保持したまま追加の平文複製なしで返す。
pub(crate) fn read_stdin_all(
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    if io::stdin().is_terminal() {
        bail!("stdin document input requires pipe or redirect input");
    }
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_from(io::stdin(), max_len, too_long_message, &session)?;
    input.into_protected_secret(&session, max_len, too_long_message)
}

/// terminal 直書きを拒否し、secret 出力経路を pipe / redirect に限定する。
pub(crate) fn write_secret_stdout(secret: &ProtectedSecret) -> Result<()> {
    if io::stdout().is_terminal() {
        bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
    }
    secret.write_to(&mut io::stdout().lock())?;
    Ok(())
}

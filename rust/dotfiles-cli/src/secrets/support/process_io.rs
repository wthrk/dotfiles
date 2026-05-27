//! process 標準入出力と制御端末を扱う共通 I/O 補助。

use std::io::{self, IsTerminal, Read, Write};

use anyhow::{Context, bail};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::Result;

use super::protection::{
    ProtectedSecret, SecretSession, buffer::ProtectedInputBuffer, secret_consumer,
};

/// 制御端末優先の reader を返し、pipe 実行時も対話入力境界を維持する。
fn stdin_or_tty_reader() -> Result<Box<dyn io::Read>> {
    if io::stdin().is_terminal() {
        Ok(Box::new(io::stdin()))
    } else {
        Ok(Box::new(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")
                .context("failed to open controlling terminal")?,
        ))
    }
}

/// 非表示入力を raw mode で読み取り、入力 bytes を保護メモリのまま返す。
///
/// backspace と Ctrl-C をここで吸収し、application 層へ端末制御詳細を漏らさない。
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
        if reader.read(&mut byte)? == 0 {
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

/// stdin 全体を保護バッファへ読み取り、追加の平文複製なしで返す。
pub(crate) fn read_stdin_all(
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    if io::stdin().is_terminal() {
        bail!("stdin document input requires pipe or redirect input");
    }
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_from(io::stdin(), max_len, too_long_message, &session)?;
    input.into_protected_secret_line(&session, max_len, too_long_message)
}

/// terminal 直書きを拒否し、secret 出力経路を pipe / redirect に限定する。
pub(crate) fn write_secret_stdout(secret: &ProtectedSecret) -> Result<()> {
    if io::stdout().is_terminal() {
        bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
    }
    secret_consumer::write_to(secret, &mut io::stdout().lock())?;
    Ok(())
}
